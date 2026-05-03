//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//!
//! 📦 Free Historical Intraday Files (Kibot)
//! You can download pre-compiled minute and tick data files for certain tickers directly — no API key required.
//! Includes one-minute and tick files for some stocks.
//! 👉 Good for: offline backtesting and research.
//!

use rust_decimal::Decimal;
use crate::{
    DailyDataProvider, IntradayDataProvider, ProviderResult, RateLimitedClient,
};
use chrono::{NaiveDate, NaiveDateTime};
use economind_core::model::{
    CandleEntry, DailyCandleEntry, Interval, Symbol,
};
use governor::Quota;
use std::num::NonZeroU32;
use std::ops::Range;

pub struct KibotClient {
    client: RateLimitedClient,

    username: String,
    password: String,

    base_url: String,
}

impl KibotClient {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("Failed to build reqwest client");

        let rate_limited = RateLimitedClient::new_with_quota(
            client,
            Quota::per_minute(NonZeroU32::new(6).unwrap()),
        );

        Self {
            client: rate_limited,

            username: username.into(),
            password: password.into(),

            base_url: "http://api.kibot.com/".to_string(),
        }
    }

    async fn fetch_url(&self, url: String) -> ProviderResult<String> {
        let resp = self.client.get(&url).await?;

        let text = resp.text().await?;

        Ok(text)
    }

    fn interval_to_kibot(interval: Interval) -> ProviderResult<String> {
        match interval {
            Interval::OneMinute => Ok("1".into()),
            Interval::FiveMinute => Ok("5".into()),
            Interval::FifteenMinute => Ok("15".into()),
            Interval::OneHour => Ok("60".into()),
            Interval::OneDay => Ok("daily".into()),
        }
    }

    fn format_date(date: NaiveDate) -> String {
        date.format("%m/%d/%Y").to_string()
    }

    fn format_datetime(dt: NaiveDateTime) -> String {
        dt.format("%m/%d/%Y").to_string()
    }
}

impl IntradayDataProvider for KibotClient {
    async fn intraday_bars(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> ProviderResult<Vec<CandleEntry>> {
        let kibot_interval = Self::interval_to_kibot(interval)?;

        let url = format!(
            "{base}?action=history&symbol={symbol}&interval={interval}&startdate={start}&enddate={end}&username={user}&password={pass}",
            base = self.base_url,
            symbol = symbol.as_str(),
            interval = kibot_interval,
            start = Self::format_datetime(time_range.start),
            end = Self::format_datetime(time_range.end),
            user = self.username,
            pass = self.password
        );

        let csv_data = self.fetch_url(url).await?;

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(csv_data.as_bytes());

        let mut entries = Vec::new();

        for record in reader.records() {
            let record = record?;

            // Expected format:
            // YYYY-MM-DD,HH:MM,Open,High,Low,Close,Volume

            let date = &record[0];
            let time = &record[1];
            let datetime =
                NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M")?;
            let open = record[2].parse::<f32>().unwrap_or(0.0);
            let high = record[3].parse::<f32>().unwrap_or(0.0);
            let low = record[4].parse::<f32>().unwrap_or(0.0);
            let close = record[5].parse::<f32>().unwrap_or(0.0);
            let volume = record[6].parse::<i32>().unwrap_or(0);

            let timestamp = datetime;

            entries.push(CandleEntry {
                timestamp,
                open: Decimal::from_f32_retain(open).unwrap_or_default(),
                high: Decimal::from_f32_retain(high).unwrap_or_default(),
                low: Decimal::from_f32_retain(low).unwrap_or_default(),
                close: Decimal::from_f32_retain(close).unwrap_or_default(),
                volume: volume as u64,
            });
        }

        Ok(entries)
    }
}

impl DailyDataProvider for KibotClient {
    async fn daily_bars(
        &self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> ProviderResult<Vec<DailyCandleEntry>> {
        let url = format!(
            "{base}?action=history&symbol={symbol}&interval=daily&startdate={start}&enddate={end}&username={user}&password={pass}",
            base = self.base_url,
            symbol = symbol.as_str(),
            start = Self::format_date(date_range.start),
            end = Self::format_date(date_range.end),
            user = self.username,
            pass = self.password
        );

        let csv_data = self.fetch_url(url).await?;

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(csv_data.as_bytes());

        let mut entries = Vec::new();

        for record in reader.records() {
            let record = record?;

            // Expected format:
            // YYYY-MM-DD,Open,High,Low,Close,Volume

            let date = NaiveDate::parse_from_str(&record[0], "%Y-%m-%d")?;

            entries.push(DailyCandleEntry {
                date,
                open: Decimal::from_f32_retain(record[1].parse().unwrap_or(0.0)).unwrap_or_default(),
                high: Decimal::from_f32_retain(record[2].parse().unwrap_or(0.0)).unwrap_or_default(),
                low: Decimal::from_f32_retain(record[3].parse().unwrap_or(0.0)).unwrap_or_default(),
                close: Decimal::from_f32_retain(record[4].parse().unwrap_or(0.0)).unwrap_or_default(),
                volume: record[5].parse().unwrap_or(0),
            });
        }

        Ok(entries)
    }
}
