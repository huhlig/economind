//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! # Providers
//! | Provider | Capabilities | Status |
//! |----------|--------------|--------|
//! | KiBot    | Historical   |        |

mod kibot;
mod rreichel;
mod tiingo;
mod traits;

pub use self::kibot::KibotClient;
pub use self::rreichel::RReichelFeed;
pub use self::tiingo::TiingoFeed;
pub use self::traits::{
    DailyDataProvider, FundamentalsProvider, IntradayDataProvider, NewsProvider, TickDataProvider,
};
