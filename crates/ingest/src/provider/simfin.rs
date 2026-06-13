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
            )) as Box<dyn std::error::Error + Send + Sync>
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
                let net_income = find_value(&stmt.data, "Net Income").unwrap_or(Decimal::ZERO);
                let interest_expense =
                    find_value(&stmt.data, "Interest Expense, Net").unwrap_or(Decimal::ZERO);
                let tax_expense = find_value(&stmt.data, "Income Tax (Expense) Benefit, Net")
                    .unwrap_or(Decimal::ZERO);
                let eps =
                    find_value(&stmt.data, "Earnings Per Share (Diluted)").unwrap_or(Decimal::ZERO);
                let ebit = op_income + interest_expense;
                results.push(IncomeStatement {
                    symbol: sym.clone(),
                    period_end,
                    period_type: "annual".to_string(),
                    revenue,
                    cogs,
                    operating_income: op_income,
                    ebit,
                    net_income,
                    eps,
                    interest_expense,
                    tax_expense,
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
                let total_assets = find_value(&stmt.data, "Total Assets").unwrap_or(Decimal::ZERO);
                let total_debt = find_value(&stmt.data, "Total Debt").unwrap_or(Decimal::ZERO);
                let total_equity = find_value(&stmt.data, "Total Equity").unwrap_or(Decimal::ZERO);
                let cash =
                    find_value(&stmt.data, "Cash & Cash Equivalents").unwrap_or(Decimal::ZERO);
                results.push(BalanceSheet {
                    symbol: sym.clone(),
                    period_end,
                    total_assets,
                    total_debt,
                    total_equity,
                    cash,
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
                let capex = find_value(&stmt.data, "Purchase of Property, Plant & Equipment")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── find_value ────────────────────────────────────────────────────────────

    fn row(concept: &str, value: &str) -> DataRow {
        DataRow {
            concept: Some(concept.to_string()),
            value: Some(value.to_string()),
        }
    }

    #[test]
    fn find_value_returns_matching_decimal() {
        let data = vec![row("Revenue", "60922000000"), row("Net Income", "29760000000")];
        let v = find_value(&data, "Revenue").unwrap();
        assert_eq!(v, Decimal::from_str("60922000000").unwrap());
    }

    #[test]
    fn find_value_returns_none_when_missing() {
        let data = vec![row("Revenue", "100")];
        assert!(find_value(&data, "Operating Income (Loss)").is_none());
    }

    #[test]
    fn find_value_returns_none_for_unparseable_number() {
        let data = vec![DataRow { concept: Some("Revenue".to_string()), value: Some("N/A".to_string()) }];
        assert!(find_value(&data, "Revenue").is_none());
    }

    // ── parse_simfin_response ─────────────────────────────────────────────────

    fn minimal_pl_response(ticker: &str, report_date: &str) -> serde_json::Value {
        json!([{
            "ticker": ticker,
            "statements": [{
                "type": "pl",
                "period": "FY",
                "fyear": 2023,
                "columns": [
                    "SimFinId", "Ticker", "Fiscal Period", "Fiscal Year",
                    "Report Date",
                    "Revenue", "Cost of Revenue", "Operating Income (Loss)",
                    "Net Income", "Interest Expense, Net",
                    "Income Tax (Expense) Benefit, Net",
                    "Earnings Per Share (Diluted)"
                ],
                "data": [[
                    "123456", ticker, "FY", 2023,
                    report_date,
                    "60922000000", "16621000000", "32972000000",
                    "29760000000", "257000000",
                    "-1041000000",
                    "11.93"
                ]]
            }]
        }])
    }

    #[test]
    fn parses_single_pl_statement() {
        let raw = minimal_pl_response("NVDA", "2023-12-31");
        let sets = parse_simfin_response(raw).unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].statements.len(), 1);
        let stmt = &sets[0].statements[0];
        assert_eq!(stmt.statement_type.as_deref(), Some("pl"));
        assert_eq!(stmt.period_end_date, "2023-12-31");
        let rev = find_value(&stmt.data, "Revenue").unwrap();
        assert_eq!(rev, Decimal::from_str("60922000000").unwrap());
    }

    #[test]
    fn parse_multiple_years() {
        let raw = json!([{
            "ticker": "AAPL",
            "statements": [{
                "type": "pl",
                "columns": ["Report Date", "Revenue"],
                "data": [
                    ["2022-09-24", "394328000000"],
                    ["2023-09-30", "383285000000"]
                ]
            }]
        }]);
        let sets = parse_simfin_response(raw).unwrap();
        assert_eq!(sets[0].statements.len(), 2);
    }

    #[test]
    fn non_array_response_returns_empty() {
        let raw = json!({ "error": "not found" });
        let sets = parse_simfin_response(raw).unwrap();
        assert!(sets.is_empty());
    }

    #[test]
    fn empty_array_returns_empty() {
        let sets = parse_simfin_response(json!([])).unwrap();
        assert!(sets.is_empty());
    }

    #[test]
    fn statement_without_statements_key_is_skipped() {
        // Company entry has no "statements" field.
        let raw = json!([{ "ticker": "AAPL" }]);
        let sets = parse_simfin_response(raw).unwrap();
        assert!(sets.is_empty());
    }

    #[test]
    fn income_statements_roundtrip_through_provider() {
        // Tests the full SimFin income_statements() parsing path (no HTTP).
        use chrono::NaiveDate;
        use economind_core::model::Symbol;

        let raw = minimal_pl_response("NVDA", "2023-12-31");
        let sets = parse_simfin_response(raw).unwrap();
        let sym = Symbol::new("NVDA");
        let mut results = Vec::new();
        for set in sets {
            for stmt in set.statements {
                if stmt.statement_type.as_deref() != Some("pl") { continue; }
                let period_end = NaiveDate::parse_from_str(&stmt.period_end_date, "%Y-%m-%d").unwrap();
                let revenue = find_value(&stmt.data, "Revenue").unwrap_or(Decimal::ZERO);
                let eps = find_value(&stmt.data, "Earnings Per Share (Diluted)").unwrap_or(Decimal::ZERO);
                results.push((sym.clone(), period_end, revenue, eps));
            }
        }
        assert_eq!(results.len(), 1);
        let (_, date, rev, eps) = &results[0];
        assert_eq!(*date, NaiveDate::from_ymd_opt(2023, 12, 31).unwrap());
        assert_eq!(*rev, Decimal::from_str("60922000000").unwrap());
        assert_eq!(*eps, Decimal::from_str("11.93").unwrap());
    }
}
