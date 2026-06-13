//! Alpaca Markets broker connector.
//!
//! Supports both paper trading (`ALPACA_PAPER=true`, default) and live trading.
//!
//! # Environment variables
//! - `ALPACA_KEY_ID` — Alpaca API key ID (required)
//! - `ALPACA_SECRET_KEY` — Alpaca API secret key (required)
//! - `ALPACA_PAPER` — `"true"` (default) for paper trading, `"false"` for live
//!
//! Paper trading base URL: `https://paper-api.alpaca.markets`
//! Live trading base URL:  `https://api.alpaca.markets`

use crate::{
    error::{BrokerError, BrokerResult},
    types::{AccountInfo, BrokerPosition, OrderRequest, OrderResult, OrderSide, OrderStatus},
    BrokerConnector,
};
use async_trait::async_trait;
use reqwest::{header, Client};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use tracing::{debug, warn};

const PAPER_BASE_URL: &str = "https://paper-api.alpaca.markets";
const LIVE_BASE_URL: &str = "https://api.alpaca.markets";

/// Alpaca Markets connector.  Clone is cheap (Arc-backed client).
#[derive(Clone)]
pub struct AlpacaConnector {
    client: Client,
    base_url: String,
}

impl AlpacaConnector {
    /// Build from environment variables.
    ///
    /// Reads `ALPACA_KEY_ID`, `ALPACA_SECRET_KEY`, and `ALPACA_PAPER`.
    pub fn from_env() -> BrokerResult<Self> {
        let key_id = std::env::var("ALPACA_KEY_ID")
            .map_err(|_| BrokerError::Api { status: 0, message: "ALPACA_KEY_ID not set".into() })?;
        let secret = std::env::var("ALPACA_SECRET_KEY").map_err(|_| BrokerError::Api {
            status: 0,
            message: "ALPACA_SECRET_KEY not set".into(),
        })?;
        let paper = std::env::var("ALPACA_PAPER")
            .map(|v| v != "false")
            .unwrap_or(true);

        Self::new(key_id, secret, paper)
    }

    /// Build with explicit credentials.
    pub fn new(key_id: String, secret_key: String, paper: bool) -> BrokerResult<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert("APCA-API-KEY-ID", header::HeaderValue::from_str(&key_id).unwrap());
        headers.insert(
            "APCA-API-SECRET-KEY",
            header::HeaderValue::from_str(&secret_key).unwrap(),
        );

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(BrokerError::Http)?;

        let base_url = if paper {
            PAPER_BASE_URL.to_string()
        } else {
            LIVE_BASE_URL.to_string()
        };

        Ok(Self { client, base_url })
    }

    async fn check_response(&self, resp: reqwest::Response) -> BrokerResult<reqwest::Response> {
        let status = resp.status();
        if status.is_success() || status.as_u16() == 204 {
            return Ok(resp);
        }
        let code = status.as_u16();
        let body = resp.text().await.unwrap_or_default();
        // Try to extract Alpaca's JSON error message.
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(String::from))
            .unwrap_or(body);
        Err(BrokerError::Api { status: code, message })
    }
}

// ── Alpaca response shapes ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AlpacaOrder {
    id: String,
    symbol: String,
    side: String,
    qty: String,
    filled_qty: String,
    filled_avg_price: Option<String>,
    status: String,
    submitted_at: String,
}

#[derive(Debug, Deserialize)]
struct AlpacaAccount {
    equity: String,
    cash: String,
    buying_power: String,
    portfolio_value: String,
}

#[derive(Debug, Deserialize)]
struct AlpacaPosition {
    symbol: String,
    qty: String,
    side: String,
    avg_entry_price: String,
    current_price: Option<String>,
    market_value: String,
    unrealized_pl: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_decimal(s: &str, field: &str) -> BrokerResult<Decimal> {
    Decimal::from_str(s).map_err(|_| BrokerError::Parse(format!("invalid {field}: {s:?}")))
}

fn parse_decimal_opt(s: Option<&str>, field: &str) -> BrokerResult<Option<Decimal>> {
    match s {
        None => Ok(None),
        Some(v) => parse_decimal(v, field).map(Some),
    }
}

fn parse_order_status(s: &str) -> OrderStatus {
    match s {
        "new" => OrderStatus::New,
        "partially_filled" => OrderStatus::PartiallyFilled,
        "filled" => OrderStatus::Filled,
        "canceled" => OrderStatus::Canceled,
        "rejected" => OrderStatus::Rejected,
        "pending_new" => OrderStatus::PendingNew,
        "pending_cancel" => OrderStatus::PendingCancel,
        "expired" => OrderStatus::Expired,
        other => OrderStatus::Unknown(other.to_string()),
    }
}

fn parse_submitted_at(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

// ── BrokerConnector impl ──────────────────────────────────────────────────────

#[async_trait]
impl BrokerConnector for AlpacaConnector {
    async fn submit_order(&self, req: OrderRequest) -> BrokerResult<OrderResult> {
        let side_str = match req.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let body = serde_json::json!({
            "symbol": req.symbol,
            "qty": req.shares.to_string(),
            "side": side_str,
            "type": "market",
            "time_in_force": "day",
        });

        debug!(symbol=%req.symbol, side=%side_str, shares=%req.shares, "Submitting order to Alpaca");

        let resp = self
            .client
            .post(format!("{}/v2/orders", self.base_url))
            .json(&body)
            .send()
            .await?;

        let resp = self.check_response(resp).await?;
        let order: AlpacaOrder = resp.json().await?;

        let status = parse_order_status(&order.status);
        if matches!(status, OrderStatus::Rejected) {
            return Err(BrokerError::Rejected(format!(
                "Order for {} rejected by Alpaca",
                req.symbol
            )));
        }

        Ok(OrderResult {
            broker_order_id: order.id,
            symbol: order.symbol,
            side: if order.side == "buy" { OrderSide::Buy } else { OrderSide::Sell },
            requested_shares: parse_decimal(&order.qty, "qty")?,
            filled_shares: parse_decimal(&order.filled_qty, "filled_qty")?,
            avg_fill_price: parse_decimal_opt(order.filled_avg_price.as_deref(), "filled_avg_price")?,
            status,
            submitted_at: parse_submitted_at(&order.submitted_at),
        })
    }

    async fn cancel_order(&self, order_id: &str) -> BrokerResult<()> {
        debug!(order_id=%order_id, "Canceling Alpaca order");
        let resp = self
            .client
            .delete(format!("{}/v2/orders/{}", self.base_url, order_id))
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    async fn get_account(&self) -> BrokerResult<AccountInfo> {
        let resp = self
            .client
            .get(format!("{}/v2/account", self.base_url))
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        let acct: AlpacaAccount = resp.json().await?;

        Ok(AccountInfo {
            equity: parse_decimal(&acct.equity, "equity")?,
            cash: parse_decimal(&acct.cash, "cash")?,
            buying_power: parse_decimal(&acct.buying_power, "buying_power")?,
            portfolio_value: parse_decimal(&acct.portfolio_value, "portfolio_value")?,
            drawdown: None, // Alpaca doesn't report drawdown directly
        })
    }

    async fn get_positions(&self) -> BrokerResult<Vec<BrokerPosition>> {
        let resp = self
            .client
            .get(format!("{}/v2/positions", self.base_url))
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        let positions: Vec<AlpacaPosition> = resp.json().await?;

        let mut out = Vec::with_capacity(positions.len());
        for p in positions {
            let current_price = match p.current_price.as_deref() {
                Some(s) => parse_decimal(s, "current_price")?,
                None => {
                    warn!(symbol=%p.symbol, "Alpaca position missing current_price");
                    Decimal::ZERO
                }
            };
            out.push(BrokerPosition {
                symbol: p.symbol,
                shares: parse_decimal(&p.qty, "qty")?,
                avg_entry_price: parse_decimal(&p.avg_entry_price, "avg_entry_price")?,
                current_price,
                market_value: parse_decimal(&p.market_value, "market_value")?,
                unrealized_pl: parse_decimal(&p.unrealized_pl, "unrealized_pl")?,
                side: p.side,
            });
        }
        Ok(out)
    }
}
