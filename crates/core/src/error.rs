//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("unsupported interval")]
    UnsupportedInterval,

    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("other error: {0}")]
    Other(String),
}
