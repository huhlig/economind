//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! FRED (Federal Reserve Economic Data) macro connector.
//!
//! API docs: <https://fred.stlouisfed.org/docs/api/fred/>
//!
//! A free API key is required — register at <https://fred.stlouisfed.org/>.
//! Pass the key via the `FRED_API_KEY` environment variable or directly to
//! [`FredConnector::new`].
//!
//! # Default series tracked
//!
//! | Series ID  | Description              |
//! |------------|--------------------------|
//! | DGS10      | 10-Year Treasury yield   |
//! | T10Y2Y     | 10Y-2Y yield curve spread|
//! | CPIAUCSL   | CPI All Urban Consumers  |
//! | UNRATE     | Unemployment rate        |
//! | VIXCLS     | CBOE VIX close           |
//! | M2SL       | M2 money supply          |

use crate::{ProviderError, ProviderResult, RateLimitedClient};
use chrono::{NaiveDate, Utc};
use economind_db::storage::{MacroSeriesPoint, MacroStorage};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

// ── Constants ─────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://api.stlouisfed.org/fred";
/// Conservative: FRED allows ~120 req/min for free keys.
const RATE_LIMIT_PER_SEC: u32 = 2;

/// Series IDs fetched by default when no override list is provided.
pub const DEFAULT_SERIES: &[&str] = &[
    "DGS10",    // 10-Year Treasury Constant Maturity Rate
    "T10Y2Y",   // 10-Year minus 2-Year Treasury yield spread
    "CPIAUCSL", // Consumer Price Index – All Urban Consumers
    "UNRATE",   // Unemployment Rate
    "VIXCLS",   // CBOE Volatility Index (VIX)
    "M2SL",     // M2 Money Supply
];

// ── Public struct ─────────────────────────────────────────────────────────────

/// FRED macro series connector.
#[derive(Clone)]
pub struct FredConnector {
    client: RateLimitedClient,
    api_key: String,
}

impl FredConnector {
    /// Create with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        let http = Client::builder()
            .user_agent("Economind/0.1")
            .build()
            .expect("reqwest client build");
        Self {
            client: RateLimitedClient::per_second(http, RATE_LIMIT_PER_SEC),
            api_key: api_key.into(),
        }
    }

    /// Create from `FRED_API_KEY` environment variable.
    pub fn from_env() -> ProviderResult<Self> {
        let key = std::env::var("FRED_API_KEY").map_err(|_| {
            Box::new(ProviderError::OtherError(
                "FRED_API_KEY environment variable not set".to_string(),
            )) as Box<dyn std::error::Error>
        })?;
        Ok(Self::new(key))
    }

    // ── Fetch single series ───────────────────────────────────────────────────

    /// Fetch all observations for a single FRED series.
    ///
    /// Returns observations in ascending date order.
    pub async fn fetch_series(
        &self,
        series_id: &str,
        since: Option<NaiveDate>,
    ) -> ProviderResult<Vec<MacroSeriesPoint>> {
        let mut url = format!(
            "{}/series/observations?series_id={}&api_key={}&file_type=json",
            BASE_URL, series_id, self.api_key,
        );
        if let Some(start) = since {
            url.push_str(&format!("&observation_start={}", start.format("%Y-%m-%d")));
        }

        let resp = self.client.get(&url).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "FRED HTTP {} for series {series_id}",
                resp.status()
            ))));
        }

        let root: FredObsRoot = resp.json().await?;
        let fetched_at = Utc::now();

        let points = root
            .observations
            .into_iter()
            .filter_map(|obs| {
                // FRED uses "." for missing values
                let value = if obs.value == "." {
                    None
                } else {
                    Decimal::from_str(&obs.value).ok()
                };
                let date = NaiveDate::parse_from_str(&obs.date, "%Y-%m-%d").ok()?;
                Some(MacroSeriesPoint {
                    series_id: series_id.to_string(),
                    date,
                    value,
                    fetched_at,
                })
            })
            .collect();

        Ok(points)
    }

    // ── Bulk fetch ────────────────────────────────────────────────────────────

    /// Fetch multiple series and write them to the provided `MacroStorage`.
    ///
    /// Uses `series_ids` if provided, otherwise falls back to [`DEFAULT_SERIES`].
    /// Errors for individual series are logged but do not abort the batch.
    pub async fn fetch_and_store<S: MacroStorage>(
        &self,
        storage: &S,
        series_ids: Option<&[&str]>,
        since: Option<NaiveDate>,
    ) -> ProviderResult<FetchStats> {
        let ids = series_ids.unwrap_or(DEFAULT_SERIES);
        let mut stats = FetchStats::default();

        for &sid in ids {
            match self.fetch_series(sid, since).await {
                Ok(points) => {
                    let n = points.len();
                    match storage.upsert_macro_series(&points).await {
                        Ok(()) => {
                            eprintln!("[fred] stored {n} observations for {sid}");
                            stats.series_ok += 1;
                            stats.points_stored += n;
                        }
                        Err(e) => {
                            eprintln!("[fred] storage error for {sid}: {e}");
                            stats.series_err += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[fred] fetch error for {sid}: {e}");
                    stats.series_err += 1;
                }
            }
        }

        Ok(stats)
    }
}

// ── Stats ─────────────────────────────────────────────────────────────────────

/// Summary of a bulk fetch operation.
#[derive(Debug, Default)]
pub struct FetchStats {
    pub series_ok: usize,
    pub series_err: usize,
    pub points_stored: usize,
}

// ── Serde types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FredObsRoot {
    observations: Vec<FredObservation>,
}

#[derive(Deserialize)]
struct FredObservation {
    date: String,
    value: String,
}
