//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::ProviderResult;
use economind_db::storage::DuckDatabase;

#[allow(dead_code)]
pub struct RReichelFeed {
    data_manager: DuckDatabase,
}

impl RReichelFeed {
    pub fn new(data_manager: DuckDatabase) -> Self {
        Self { data_manager }
    }

    pub async fn upsert_tickers(&self) -> ProviderResult<()> {
        // TODO: Implement ingestion from RReichel JSON sources and map into datamodel types
        Ok(())
    }
}
