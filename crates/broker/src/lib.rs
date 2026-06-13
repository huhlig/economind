//! Economind broker connectors.
//!
//! Provides the `BrokerConnector` trait and concrete implementations for
//! supported brokers.  All implementations default to paper trading — live
//! trading requires explicit configuration (`execution_mode = "live"`).
//!
//! # Supported brokers
//! - **Alpaca** — `AlpacaConnector` (paper: `paper-api.alpaca.markets`)

pub mod alpaca;
pub mod error;
pub mod types;

pub use alpaca::AlpacaConnector;
pub use error::{BrokerError, BrokerResult};
pub use types::{AccountInfo, BrokerPosition, OrderRequest, OrderResult, OrderSide, OrderStatus};

use async_trait::async_trait;

/// Core trait that all broker connectors implement.
///
/// All methods are async and return `BrokerResult<T>`.
/// Implementors should be cheaply cloneable (use `Arc` internally for shared state).
#[async_trait]
pub trait BrokerConnector: Send + Sync {
    /// Submit a market order.  Returns the broker's order confirmation.
    async fn submit_order(&self, req: OrderRequest) -> BrokerResult<OrderResult>;

    /// Cancel an open order by broker order ID.
    async fn cancel_order(&self, order_id: &str) -> BrokerResult<()>;

    /// Fetch current account summary (equity, cash, buying power).
    async fn get_account(&self) -> BrokerResult<AccountInfo>;

    /// Fetch all open positions.
    async fn get_positions(&self) -> BrokerResult<Vec<BrokerPosition>>;
}
