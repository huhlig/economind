//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
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

    #[error("unsupported interval")]
    UnsupportedInterval,

    #[error("provider error: {0}")]
    Provider(String),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Database error: {0}")]
    DatabaseError(#[from] duckdb::Error),

    #[error("Parquet error: {0}")]
    ParquetError(String),

    #[error("Arrow error: {0}")]
    ArrowError(String),
}
