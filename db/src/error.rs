//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

use thiserror::Error;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("network error: {0}")]
    Network(String),

    #[error("rate limit exceeded")]
    RateLimited,

    #[error("symbol not found")]
    SymbolNotFound,

    #[error("record not found")]
    NotFound,

    #[error("unsupported interval")]
    UnsupportedInterval,

    #[error("provider error: {0}")]
    Provider(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),

    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] sqlx::Error),

    #[error("Parquet error: {0}")]
    Parquet(String),
}
