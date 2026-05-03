//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! SimFin fundamentals connector.
//!
//! SimFin provides free standardized annual and quarterly financial statements.
//! Register at <https://simfin.com/> to get a free API key.
//! Pass the key via `SIMFIN_API_KEY` env var or directly to [`SimFinConnector::new`].
//!
//! API base: `https://backend.simfin.com/api/v3/`
//!
//! # Rate limits
//! Free tier: 5 req/s (undocumented but observed). We use 2 req/s to be safe.
//!
//! # Endpoints used
//! - `/companies/statements/bulk?ticker=<T>&statements=pl,bs,cf&period=FY`
//!   Fetches annual P&L, Balance Sheet, Cash Flow for a single ticker.

use crate::{FundamentalsProvider, ProviderError, ProviderResult, RateLimitedClient};
use chrono::NaiveDate;
use economind_core::model::{BalanceSheet, CashFlowStatement, IncomeStatement, Symbol};
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

// ── Constants ─────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://backend.simfin.com/api/v3";
const RATE_LIMIT_PER_SEC: u32 = 2;

// ── Connector ─────────────────────────────────────────────────────────────────

/// SimFin fundamentals connector.
#[derive(Clone)]
pub struct SimFinConnector {
    client: RateLimitedClient,
    api_key: String,
}

impl SimFinConnector {
    /// Create with an explicit API key.
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

    /// Create from `SIMFIN_API_KEY` environment variable.
    pub fn from_env() -> ProviderResult<Self> {
        let key = std::env::var("SIMFIN_API_KEY").map_err(|_| {
            Box::new(ProviderError::OtherError(
                "SIMFIN_API_KEY environment variable not set".to_string(),
            )) as Box<dyn std::error::Error>
        })?;
        Ok(Self::new(key))
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Fetch the raw SimFin statement bulk response for `ticker`.
    ///
    /// SimFin v3 returns columnar data: each statement has a `columns` array
    /// and a `data` array-of-arrays.  We zip columns with row values to produce
    /// a flat `Vec<DataRow>` for downstream processing.
    async fn fetch_statements(&self, ticker: &str) -> ProviderResult<Vec<StatementSet>> {
        let url = format!(
            "{}/companies/statements/bulk?ticker={}&statements=pl,bs,cf&period=FY&api-key={}",
            BASE_URL, ticker, self.api_key,
        );
        let resp = self.client.get(&url).await?;
        if !resp.status().is_success() {
            return Err(Box::new(ProviderError::OtherError(format!(
                "SimFin HTTP {} for ticker {ticker}",
                resp.status()
            ))));
        }

        // Parse as generic JSON first so we can handle either response shape.
        let raw: serde_json::Value = resp.json().await?;
        parse_simfin_response(raw)
    }
}

impl FundamentalsProvider for SimFinConnector {
    async fn income_statements(&self, symbol: &str) -> ProviderResult<Vec<IncomeStatement>> {
        let sets = self.fetch_statements(symbol).await?;
        let sym = Symbol::new(symbol);
        let mut results = Vec::new();

        for set in sets {
            // Each set covers one fiscal year; find the P&L statement.
            for stmt in set.statements {
                if stmt.statement_type.as_deref() != Some("pl") {
                    continue;
                }
                let period_end = match NaiveDate::parse_from_str(&stmt.period_end_date, "%Y-%m-%d")
                {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let revenue = find_value(&stmt.data, "Revenue").unwrap_or(Decimal::ZERO);
                let cogs = find_value(&stmt.data, "Cost of Revenue").unwrap_or(Decimal::ZERO);
                let op_income =
                    find_value(&stmt.data, "Operating Income (Loss)").unwrap_or(Decimal::ZERO);
                results.push(IncomeStatement {
                    symbol: sym.clone(),
                    period_end,
                    revenue,
                    cogs,
                    operating_income: op_income,
                });
            }
        }
        results.sort_by_key(|s| s.period_end);
        Ok(results)
    }

    async fn balance_sheets(&self, symbol: &str) -> ProviderResult<Vec<BalanceSheet>> {
        let sets = self.fetch_statements(symbol).await?;
        let sym = Symbol::new(symbol);
        let mut results = Vec::new();

        for set in sets {
            for stmt in set.statements {
                if stmt.statement_type.as_deref() != Some("bs") {
                    continue;
                }
                let period_end = match NaiveDate::parse_from_str(&stmt.period_end_date, "%Y-%m-%d")
                {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let total_assets =
                    find_value(&stmt.data, "Total Assets").unwrap_or(Decimal::ZERO);
                let total_debt =
                    find_value(&stmt.data, "Total Debt").unwrap_or(Decimal::ZERO);
                let total_equity =
                    find_value(&stmt.data, "Total Equity").unwrap_or(Decimal::ZERO);
                results.push(BalanceSheet {
                    symbol: sym.clone(),
                    period_end,
                    total_assets,
                    total_debt,
                    total_equity,
                });
            }
        }
        results.sort_by_key(|s| s.period_end);
        Ok(results)
    }

    async fn cash_flows(&self, symbol: &str) -> ProviderResult<Vec<CashFlowStatement>> {
        let sets = self.fetch_statements(symbol).await?;
        let sym = Symbol::new(symbol);
        let mut results = Vec::new();

        for set in sets {
            for stmt in set.statements {
                if stmt.statement_type.as_deref() != Some("cf") {
                    continue;
                }
                let period_end = match NaiveDate::parse_from_str(&stmt.period_end_date, "%Y-%m-%d")
                {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let op_cf = find_value(&stmt.data, "Net Cash from Operating Activities")
                    .unwrap_or(Decimal::ZERO);
                let capex = find_value(
                    &stmt.data,
                    "Purchase of Property, Plant & Equipment",
                )
                .unwrap_or(Decimal::ZERO)
                .abs();
                results.push(CashFlowStatement {
                    symbol: sym.clone(),
                    period_end,
                    operating_cash_flow: op_cf,
                    capex,
                });
            }
        }
        results.sort_by_key(|s| s.period_end);
        Ok(results)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Find a named line item in the data rows and return its value.
fn find_value(data: &[DataRow], name: &str) -> Option<Decimal> {
    data.iter()
        .find(|r| r.concept.as_deref() == Some(name))
        .and_then(|r| r.value.as_deref())
        .and_then(|v| Decimal::from_str(v).ok())
}

// ── Response parser ───────────────────────────────────────────────────────────

/// Parse the SimFin v3 JSON response into our internal `StatementSet` list.
///
/// SimFin v3 columnar format:
/// ```json
/// [
///   {
///     "ticker": "AAPL",
///     "statements": [
///       {
///         "type": "pl",
///         "period": "FY",
///         "fyear": 2023,
///         "columns": ["SimFinId", "Ticker", "Fiscal Period", "Fiscal Year",
///                     "Report Date", "Publish Date", "Restated Date",
///                     "Shares (Basic)", "Shares (Diluted)",
///                     "Revenue", "Cost of Revenue", "Operating Income (Loss)", ...],
///         "data": [[...], [...]]
///       }
///     ]
///   }
/// ]
/// ```
fn parse_simfin_response(raw: Value) -> ProviderResult<Vec<StatementSet>> {
    // The outer array may contain one item per ticker; we asked for one ticker.
    let outer = match raw.as_array() {
        Some(a) => a.clone(),
        None => {
            // Some endpoints wrap in an object — try {"data": [...]} shape too.
            return Ok(vec![]);
        }
    };

    let mut sets: Vec<StatementSet> = Vec::new();

    for company in outer {
        let stmts_json = match company.get("statements").and_then(|s| s.as_array()) {
            Some(a) => a.clone(),
            None => continue,
        };

        let mut statements: Vec<Statement> = Vec::new();

        for stmt_json in stmts_json {
            let stmt_type = stmt_json
                .get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let columns: Vec<String> = stmt_json
                .get("columns")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_str().unwrap_or("").to_string())
                        .collect()
                })
                .unwrap_or_default();

            let data_rows = stmt_json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            // Find the index of "Report Date" for period_end_date.
            let date_col = columns.iter().position(|c| c == "Report Date");

            for row in data_rows {
                let row_arr = match row.as_array() {
                    Some(a) => a.clone(),
                    None => continue,
                };

                let period_end_date = date_col
                    .and_then(|i| row_arr.get(i))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Zip columns with values to produce DataRow list.
                let data: Vec<DataRow> = columns
                    .iter()
                    .zip(row_arr.iter())
                    .map(|(col, val)| DataRow {
                        concept: Some(col.clone()),
                        value: val.as_str().map(|s| s.to_string()).or_else(|| {
                            // Handle numeric values stored as JSON numbers.
                            if val.is_number() {
                                Some(val.to_string())
                            } else {
                                None
                            }
                        }),
                    })
                    .collect();

                statements.push(Statement {
                    statement_type: stmt_type.clone(),
                    period_end_date,
                    data,
                });
            }
        }

        if !statements.is_empty() {
            sets.push(StatementSet { statements });
        }
    }

    Ok(sets)
}

// ── Internal types ────────────────────────────────────────────────────────────

/// Internal representation of a statement set for one fiscal period.
struct StatementSet {
    statements: Vec<Statement>,
}

struct Statement {
    statement_type: Option<String>,
    period_end_date: String,
    data: Vec<DataRow>,
}

struct DataRow {
    concept: Option<String>,
    value: Option<String>,
}
