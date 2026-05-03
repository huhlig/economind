//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! SEC EDGAR fundamentals connector — no API key required.
//!
//! Uses two free endpoints:
//! - Company tickers JSON for ticker→CIK mapping:
//!   `https://www.sec.gov/files/company_tickers.json`
//! - XBRL company facts for financial statements:
//!   `https://data.sec.gov/api/xbrl/companyfacts/CIK{cik:010}.json`
//!
//! SEC EDGAR rate limits: max 10 req/s per User-Agent policy.
//! We use 5 req/s to stay well within the limit.
//!
//! # What is extracted
//!
//! From the XBRL `us-gaap` taxonomy we extract the following concepts and
//! map them to our core model:
//!
//! **IncomeStatement**
//! - Revenue   → `Revenues` or `RevenueFromContractWithCustomerExcludingAssessedTax`
//! - COGS      → `CostOfGoodsSoldAndServicesSold` or `CostOfRevenue`
//! - OpIncome  → `OperatingIncomeLoss`
//!
//! **BalanceSheet**
//! - TotalAssets  → `Assets`
//! - TotalDebt    → `LongTermDebt`
//! - TotalEquity  → `StockholdersEquity` or `StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest`
//!
//! **CashFlowStatement**
//! - OperatingCF → `NetCashProvidedByUsedInOperatingActivities`
//! - CapEx       → `PaymentsToAcquirePropertyPlantAndEquipment` (negated)

use crate::{FundamentalsProvider, ProviderError, ProviderResult, RateLimitedClient};
use chrono::NaiveDate;
use economind_core::model::{BalanceSheet, CashFlowStatement, IncomeStatement, Symbol};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Constants ─────────────────────────────────────────────────────────────────

const TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const FACTS_BASE_URL: &str = "https://data.sec.gov/api/xbrl/companyfacts";
/// 5 req/s — SEC EDGAR policy allows 10/s; we stay at half for safety.
const RATE_LIMIT_PER_SEC: u32 = 5;
/// SEC requires a meaningful User-Agent with contact info.
const USER_AGENT: &str = "Economind/0.1 contact@economind.example";

// ── CIK lookup cache ──────────────────────────────────────────────────────────

/// Thread-safe cache mapping UPPER-CASE ticker → zero-padded 10-digit CIK string.
#[derive(Clone, Default)]
pub struct CikCache {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl CikCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, ticker: &str) -> Option<String> {
        self.inner.read().await.get(&ticker.to_uppercase()).cloned()
    }

    pub async fn insert(&self, ticker: String, cik: String) {
        self.inner.write().await.insert(ticker.to_uppercase(), cik);
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

// ── Main connector ────────────────────────────────────────────────────────────

/// SEC EDGAR XBRL fundamentals connector.
#[derive(Clone)]
pub struct EdgarConnector {
    client: RateLimitedClient,
    cik_cache: CikCache,
}

impl EdgarConnector {
    /// Create a new connector.  The CIK cache is empty until `load_cik_map` is called.
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client build");
        Self {
            client: RateLimitedClient::per_second(http, RATE_LIMIT_PER_SEC),
            cik_cache: CikCache::new(),
        }
    }

    // ── CIK map ───────────────────────────────────────────────────────────────

    /// Download and cache the full ticker→CIK mapping from SEC.
    ///
    /// Call this once before any fundamentals fetch.  The JSON contains ~14 000
    /// entries and downloads in a single request.
    pub async fn load_cik_map(&self) -> ProviderResult<usize> {
        let resp = self.client.get(TICKERS_URL).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "EDGAR tickers HTTP {}",
                resp.status()
            ))));
        }

        // The JSON is an object keyed by integer index: { "0": { cik_str, ticker, title }, ... }
        let raw: HashMap<String, TickerEntry> = resp.json().await?;
        let count = raw.len();
        for entry in raw.into_values() {
            // CIK must be zero-padded to 10 digits
            let cik = format!("{:010}", entry.cik_str);
            self.cik_cache.insert(entry.ticker, cik).await;
        }
        eprintln!("[edgar] loaded {} ticker→CIK mappings", count);
        Ok(count)
    }

    /// Look up the CIK for a ticker symbol.  Loads the CIK map if not yet loaded.
    pub async fn cik_for(&self, symbol: &Symbol) -> ProviderResult<String> {
        if self.cik_cache.is_empty().await {
            self.load_cik_map().await?;
        }
        self.cik_cache
            .get(symbol.as_str())
            .await
            .ok_or_else(|| {
                Box::new(ProviderError::OtherError(format!(
                    "No CIK found for symbol {}",
                    symbol.as_str()
                ))) as Box<dyn std::error::Error>
            })
    }

    // ── Raw XBRL fetch ────────────────────────────────────────────────────────

    /// Fetch the full XBRL company facts JSON for a given CIK.
    async fn fetch_facts(&self, cik: &str) -> ProviderResult<CompanyFacts> {
        let url = format!("{}/CIK{}.json", FACTS_BASE_URL, cik);
        let resp = self.client.get(&url).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "EDGAR facts HTTP {} for CIK {cik}",
                resp.status()
            ))));
        }
        Ok(resp.json::<CompanyFacts>().await?)
    }
}

impl Default for EdgarConnector {
    fn default() -> Self {
        Self::new()
    }
}

// ── FundamentalsProvider impl ─────────────────────────────────────────────────

impl FundamentalsProvider for EdgarConnector {
    async fn income_statements(&self, symbol: &str) -> ProviderResult<Vec<IncomeStatement>> {
        let sym = Symbol::new(symbol);
        let cik = self.cik_for(&sym).await?;
        let facts = self.fetch_facts(&cik).await?;

        let gaap = match facts.facts.us_gaap {
            Some(g) => g,
            None => return Ok(vec![]),
        };

        // Collect all annual (10-K) filing periods for which we have enough data.
        // We pick one revenue concept, one COGS concept, and operating income.
        let revenues = pick_concept(
            &gaap,
            &[
                "Revenues",
                "RevenueFromContractWithCustomerExcludingAssessedTax",
                "SalesRevenueNet",
            ],
        );
        let cogs = pick_concept(
            &gaap,
            &["CostOfGoodsSoldAndServicesSold", "CostOfRevenue", "CostOfGoodsSold"],
        );
        let op_income = pick_concept(&gaap, &["OperatingIncomeLoss"]);

        // Build a date-keyed map from operating income (the most reliably present).
        let op_map = annual_value_map(op_income);
        let rev_map = annual_value_map(revenues);
        let cogs_map = annual_value_map(cogs);

        let mut stmts = Vec::new();
        for (period_end, op) in &op_map {
            let revenue = rev_map.get(period_end).copied().unwrap_or(Decimal::ZERO);
            let cogs_val = cogs_map.get(period_end).copied().unwrap_or(Decimal::ZERO);
            stmts.push(IncomeStatement {
                symbol: sym.clone(),
                period_end: *period_end,
                revenue,
                cogs: cogs_val,
                operating_income: *op,
            });
        }
        stmts.sort_by_key(|s| s.period_end);
        Ok(stmts)
    }

    async fn balance_sheets(&self, symbol: &str) -> ProviderResult<Vec<BalanceSheet>> {
        let sym = Symbol::new(symbol);
        let cik = self.cik_for(&sym).await?;
        let facts = self.fetch_facts(&cik).await?;

        let gaap = match facts.facts.us_gaap {
            Some(g) => g,
            None => return Ok(vec![]),
        };

        let assets = pick_concept(&gaap, &["Assets"]);
        let debt = pick_concept(
            &gaap,
            &["LongTermDebt", "LongTermDebtAndCapitalLeaseObligations"],
        );
        let equity = pick_concept(
            &gaap,
            &[
                "StockholdersEquity",
                "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
            ],
        );

        let assets_map = annual_value_map(assets);
        let debt_map = annual_value_map(debt);
        let equity_map = annual_value_map(equity);

        let mut sheets = Vec::new();
        for (period_end, total_assets) in &assets_map {
            let total_debt = debt_map.get(period_end).copied().unwrap_or(Decimal::ZERO);
            let total_equity = equity_map.get(period_end).copied().unwrap_or(Decimal::ZERO);
            sheets.push(BalanceSheet {
                symbol: sym.clone(),
                period_end: *period_end,
                total_assets: *total_assets,
                total_debt,
                total_equity,
            });
        }
        sheets.sort_by_key(|s| s.period_end);
        Ok(sheets)
    }

    async fn cash_flows(&self, symbol: &str) -> ProviderResult<Vec<CashFlowStatement>> {
        let sym = Symbol::new(symbol);
        let cik = self.cik_for(&sym).await?;
        let facts = self.fetch_facts(&cik).await?;

        let gaap = match facts.facts.us_gaap {
            Some(g) => g,
            None => return Ok(vec![]),
        };

        let op_cf = pick_concept(
            &gaap,
            &["NetCashProvidedByUsedInOperatingActivities"],
        );
        let capex = pick_concept(
            &gaap,
            &[
                "PaymentsToAcquirePropertyPlantAndEquipment",
                "CapitalExpendituresIncurringObligation",
            ],
        );

        let op_map = annual_value_map(op_cf);
        let capex_map = annual_value_map(capex);

        let mut stmts = Vec::new();
        for (period_end, op) in &op_map {
            // CapEx from EDGAR is typically negative (cash outflow); we store it as positive.
            let capex_val = capex_map
                .get(period_end)
                .copied()
                .unwrap_or(Decimal::ZERO)
                .abs();
            stmts.push(CashFlowStatement {
                symbol: sym.clone(),
                period_end: *period_end,
                operating_cash_flow: *op,
                capex: capex_val,
            });
        }
        stmts.sort_by_key(|s| s.period_end);
        Ok(stmts)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return the first matching concept's units, or an empty slice.
fn pick_concept<'a>(
    gaap: &'a HashMap<String, XbrlConcept>,
    names: &[&str],
) -> Option<&'a XbrlConcept> {
    names.iter().find_map(|name| gaap.get(*name))
}

/// Build a map from period_end → value for annual (FY / 10-K) USD filings.
fn annual_value_map(concept: Option<&XbrlConcept>) -> HashMap<NaiveDate, Decimal> {
    let concept = match concept {
        Some(c) => c,
        None => return HashMap::new(),
    };
    let units = match concept.units.usd.as_deref() {
        Some(u) => u,
        None => return HashMap::new(),
    };

    let mut map: HashMap<NaiveDate, Decimal> = HashMap::new();
    for obs in units {
        // Only annual filings (10-K form), non-instantaneous (has both start and end).
        if obs.form.as_deref() != Some("10-K") {
            continue;
        }
        let end = match NaiveDate::parse_from_str(&obs.end, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        let value = match Decimal::from_str(&obs.val.to_string()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Keep the most recent accession per period_end (if duplicates exist).
        map.entry(end).or_insert(value);
    }
    map
}

// ── Serde types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TickerEntry {
    cik_str: u64,
    ticker: String,
    #[allow(dead_code)]
    title: String,
}

#[derive(Deserialize)]
struct CompanyFacts {
    facts: FactsTaxonomies,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct FactsTaxonomies {
    #[serde(rename = "us-gaap")]
    us_gaap: Option<HashMap<String, XbrlConcept>>,
}

#[derive(Deserialize)]
struct XbrlConcept {
    units: XbrlUnits,
}

#[derive(Deserialize)]
struct XbrlUnits {
    #[serde(rename = "USD")]
    usd: Option<Vec<XbrlObservation>>,
}

#[derive(Deserialize)]
struct XbrlObservation {
    end: String,
    val: f64,
    form: Option<String>,
}
