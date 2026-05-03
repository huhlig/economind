//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! Yahoo Finance connector — no API key required.
//!
//! Uses the unofficial v8 chart endpoint:
//!   `https://query1.finance.yahoo.com/v8/finance/chart/{symbol}`
//!
//! Rate limit: 2 req/s (conservative, unauthenticated tier).
//!
//! Capabilities:
//! - Daily OHLCV bars via [`DailyDataProvider`]
//! - Instrument metadata (name, sector, market cap) via [`YahooFinanceConnector::fetch_metadata`]
//! - Bulk download with configurable concurrency via [`YahooFinanceConnector::fetch_all`]

use crate::{DailyDataProvider, ProviderError, ProviderResult, RateLimitedClient};
use chrono::{NaiveDate, TimeZone, Utc};
use economind_core::model::{DailyCandleEntry, Symbol, Ticker};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::ops::Range;

// ── Constants ─────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://query1.finance.yahoo.com";
/// Requests per second — conservative to avoid 429s on the unauthenticated tier.
const RATE_LIMIT_PER_SEC: u32 = 2;
/// Default concurrency for bulk downloads.
const DEFAULT_CONCURRENCY: usize = 4;

// ── Public struct ─────────────────────────────────────────────────────────────

/// Yahoo Finance data connector.
#[derive(Clone)]
pub struct YahooFinanceConnector {
    client: RateLimitedClient,
    concurrency: usize,
}

impl YahooFinanceConnector {
    /// Create with default rate limits (2 req/s) and concurrency (4).
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; Economind/0.1)")
            .build()
            .expect("reqwest client build");
        Self {
            client: RateLimitedClient::per_second(http, RATE_LIMIT_PER_SEC),
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    /// Override the download concurrency (number of symbols fetched in parallel).
    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.concurrency = n;
        self
    }

    // ── Metadata ──────────────────────────────────────────────────────────────

    /// Fetch instrument metadata from the Yahoo Finance quote summary endpoint.
    ///
    /// Populates `Ticker.name`, `Ticker.sector`, `Ticker.marketcap`.
    pub async fn fetch_metadata(&self, symbol: &Symbol) -> ProviderResult<Ticker> {
        let url = format!(
            "{}/v10/finance/quoteSummary/{}?modules=assetProfile,summaryDetail",
            BASE_URL,
            symbol.as_str()
        );
        let resp = self.client.get(&url).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "Yahoo metadata HTTP {}: {}",
                resp.status(),
                symbol.as_str()
            ))));
        }
        let root: QuoteSummaryRoot = resp.json().await?;
        let result = root
            .quote_summary
            .result
            .into_iter()
            .next()
            .ok_or_else(|| {
                ProviderError::OtherError(format!(
                    "No quote summary result for {}",
                    symbol.as_str()
                ))
            })?;

        let name = result
            .asset_profile
            .as_ref()
            .and_then(|p| p.long_name.clone());
        let sector = result
            .asset_profile
            .as_ref()
            .and_then(|p| p.sector.clone());
        let description = result
            .asset_profile
            .as_ref()
            .and_then(|p| p.long_business_summary.clone());
        let marketcap = result
            .summary_detail
            .as_ref()
            .and_then(|d| d.market_cap.and_then(|v| Decimal::try_from(v).ok()));

        Ok(Ticker {
            symbol: symbol.clone(),
            exchange: None,
            name,
            country: result
                .asset_profile
                .as_ref()
                .and_then(|p| p.country.clone()),
            industry: None, // Industry enum not yet populated
            sector: None,   // Sector enum not yet populated
            ipoyear: None,
            marketcap,
            description,
            active: true,
        })
    }

    // ── Bulk download ─────────────────────────────────────────────────────────

    /// Download daily bars for many symbols concurrently, returning all results.
    ///
    /// Errors for individual symbols are logged but do not abort the batch;
    /// the successful entries are returned.
    pub async fn fetch_all(
        &self,
        symbols: &[Symbol],
        date_range: Range<NaiveDate>,
    ) -> Vec<(Symbol, Vec<DailyCandleEntry>)> {
        let connector = self.clone();
        stream::iter(symbols.iter().cloned())
            .map(|sym| {
                let c = connector.clone();
                let dr = date_range.clone();
                async move {
                    match c.daily_bars(&sym, dr).await {
                        Ok(bars) => {
                            tracing_log(&sym, bars.len());
                            Some((sym, bars))
                        }
                        Err(e) => {
                            eprintln!("[yahoo] fetch_all error for {}: {e}", sym.as_str());
                            None
                        }
                    }
                }
            })
            .buffer_unordered(self.concurrency)
            .filter_map(|opt| async move { opt })
            .collect()
            .await
    }
}

impl Default for YahooFinanceConnector {
    fn default() -> Self {
        Self::new()
    }
}

// ── DailyDataProvider impl ────────────────────────────────────────────────────

impl DailyDataProvider for YahooFinanceConnector {
    async fn daily_bars(
        &self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> ProviderResult<Vec<DailyCandleEntry>> {
        // Convert NaiveDate → Unix timestamps (start of day UTC)
        let period1 = date_range
            .start
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Utc.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);
        let period2 = date_range
            .end
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Utc.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        let url = format!(
            "{}/v8/finance/chart/{}?interval=1d&period1={}&period2={}&events=history",
            BASE_URL,
            symbol.as_str(),
            period1,
            period2,
        );

        let resp = self.client.get(&url).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "Yahoo chart HTTP {}: {}",
                resp.status(),
                symbol.as_str()
            ))));
        }

        let root: ChartRoot = resp.json().await?;
        let result = root
            .chart
            .result
            .into_iter()
            .next()
            .ok_or_else(|| {
                ProviderError::OtherError(format!(
                    "Yahoo returned empty result for {}",
                    symbol.as_str()
                ))
            })?;

        parse_chart_result(result)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_chart_result(result: ChartResult) -> ProviderResult<Vec<DailyCandleEntry>> {
    let timestamps = result.timestamp.unwrap_or_default();
    let quote = result
        .indicators
        .quote
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::OtherError("No quote indicators in chart result".into()))?;
    let adjclose_list = result
        .indicators
        .adjclose
        .unwrap_or_default()
        .into_iter()
        .next()
        .map(|ac| ac.adjclose)
        .unwrap_or_default();

    let opens = quote.open;
    let highs = quote.high;
    let lows = quote.low;
    let closes = quote.close;
    let volumes = quote.volume;

    let len = timestamps.len();
    let mut entries = Vec::with_capacity(len);

    for i in 0..len {
        // Skip bars where any core field is None (Yahoo sometimes returns nulls
        // for partial trading days or extended-hours-only sessions).
        let ts = timestamps[i];
        let open = opens.get(i).and_then(|v| *v);
        let high = highs.get(i).and_then(|v| *v);
        let low = lows.get(i).and_then(|v| *v);
        let close = closes.get(i).and_then(|v| *v);
        let volume = volumes.get(i).and_then(|v| *v).unwrap_or(0);
        // Use adjusted close when available, fall back to raw close.
        let _adjclose = adjclose_list.get(i).and_then(|v| *v);

        let (open, high, low, close) = match (open, high, low, close) {
            (Some(o), Some(h), Some(l), Some(c)) => (o, h, l, c),
            _ => continue,
        };

        // Yahoo timestamps are Unix seconds; convert to NaiveDate in UTC.
        let date = match Utc.timestamp_opt(ts, 0).single() {
            Some(dt) => dt.date_naive(),
            None => continue,
        };

        entries.push(DailyCandleEntry {
            date,
            open: f64_to_decimal(open)?,
            high: f64_to_decimal(high)?,
            low: f64_to_decimal(low)?,
            close: f64_to_decimal(close)?,
            volume: volume as u64,
        });
    }

    Ok(entries)
}

fn f64_to_decimal(v: f64) -> ProviderResult<Decimal> {
    Decimal::try_from(v).map_err(|e| {
        Box::new(ProviderError::OtherError(format!(
            "f64→Decimal conversion error: {e}"
        ))) as Box<dyn std::error::Error>
    })
}

#[inline]
fn tracing_log(sym: &Symbol, count: usize) {
    eprintln!("[yahoo] fetched {} bars for {}", count, sym.as_str());
}

// ── Serde types ───────────────────────────────────────────────────────────────

// ---- Chart v8 ----

#[derive(Deserialize)]
struct ChartRoot {
    chart: ChartWrapper,
}

#[derive(Deserialize)]
struct ChartWrapper {
    result: Vec<ChartResult>,
}

#[derive(Deserialize)]
struct ChartResult {
    timestamp: Option<Vec<i64>>,
    indicators: Indicators,
}

#[derive(Deserialize)]
struct Indicators {
    quote: Vec<QuoteBlock>,
    adjclose: Option<Vec<AdjCloseBlock>>,
}

#[derive(Deserialize)]
struct QuoteBlock {
    open: Vec<Option<f64>>,
    high: Vec<Option<f64>>,
    low: Vec<Option<f64>>,
    close: Vec<Option<f64>>,
    volume: Vec<Option<i64>>,
}

#[derive(Deserialize)]
struct AdjCloseBlock {
    adjclose: Vec<Option<f64>>,
}

// ---- Quote Summary v10 ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteSummaryRoot {
    quote_summary: QuoteSummaryWrapper,
}

#[derive(Deserialize)]
struct QuoteSummaryWrapper {
    result: Vec<QuoteSummaryResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteSummaryResult {
    asset_profile: Option<AssetProfile>,
    summary_detail: Option<SummaryDetail>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetProfile {
    long_name: Option<String>,
    sector: Option<String>,
    country: Option<String>,
    long_business_summary: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryDetail {
    market_cap: Option<f64>,
}
