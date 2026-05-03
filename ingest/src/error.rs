//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

pub type ProviderResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("rate limiter closed")]
    RateLimiterClosed,
    #[error("chrono parse error: {0}")]
    ChronoParseError(#[from] chrono::ParseError),
    #[error("csv error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("http error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("other error: {0}")]
    OtherError(String),
}
