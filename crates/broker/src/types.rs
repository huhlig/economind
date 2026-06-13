use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which side of the market the order is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "buy"),
            OrderSide::Sell => write!(f, "sell"),
        }
    }
}

/// Status of an order as reported by the broker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    PendingNew,
    PendingCancel,
    Expired,
    Unknown(String),
}

/// A request to submit a market order.
#[derive(Debug, Clone)]
pub struct OrderRequest {
    /// Internal signal ID this order originates from.
    pub signal_id: Uuid,
    /// Ticker symbol (e.g. "AAPL").
    pub symbol: String,
    /// Buy (entering long / exiting short) or Sell (entering short / exiting long).
    pub side: OrderSide,
    /// Number of shares.  Must be positive.
    pub shares: Decimal,
    /// Human-readable note attached to the order for audit purposes.
    pub note: Option<String>,
}

/// Broker's confirmation of a submitted order.
#[derive(Debug, Clone)]
pub struct OrderResult {
    /// Broker-assigned order ID.
    pub broker_order_id: String,
    /// Symbol the order was placed for.
    pub symbol: String,
    pub side: OrderSide,
    pub requested_shares: Decimal,
    pub filled_shares: Decimal,
    pub avg_fill_price: Option<Decimal>,
    pub status: OrderStatus,
    pub submitted_at: DateTime<Utc>,
}

/// Account summary returned by the broker.
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub equity: Decimal,
    pub cash: Decimal,
    pub buying_power: Decimal,
    pub portfolio_value: Decimal,
    /// Fractional drawdown from equity peak (0.0 – 1.0).  None if broker doesn't report it.
    pub drawdown: Option<Decimal>,
}

/// A single open position as reported by the broker.
#[derive(Debug, Clone)]
pub struct BrokerPosition {
    pub symbol: String,
    pub shares: Decimal,
    pub avg_entry_price: Decimal,
    pub current_price: Decimal,
    pub market_value: Decimal,
    pub unrealized_pl: Decimal,
    /// "long" or "short"
    pub side: String,
}
