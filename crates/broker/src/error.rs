use thiserror::Error;

pub type BrokerResult<T> = Result<T, BrokerError>;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Broker API error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Order rejected: {0}")]
    Rejected(String),

    #[error("Risk limit: {0}")]
    RiskLimit(String),
}
