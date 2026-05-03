//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::ProviderResult;
use chrono::NaiveDate;
use economind_db::storage::DuckDatabase;

#[allow(dead_code)]
pub struct TiingoFeed {
    data_manager: DuckDatabase,
    api_key: String,
}

impl TiingoFeed {
    pub fn new(data_manager: DuckDatabase, api_key: String) -> Self {
        Self { data_manager, api_key }
    }

    pub async fn fetch_ticker_metadata(&self, _ticker: &str) -> ProviderResult<()> {
        // TODO: Implement Tiingo metadata fetch mapping into datamodel types
        Ok(())
    }

    pub async fn fetch_ticker_prices(
        &self,
        _ticker: &str,
        _date_range: Option<std::ops::Range<NaiveDate>>,
    ) -> ProviderResult<()> {
        // TODO: Implement Tiingo daily prices fetch mapping into datamodel types
        Ok(())
    }
}
