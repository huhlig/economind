//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! # Data Providers
//!
//! | Provider  | Capabilities                | Auth       | Status     |
//! |-----------|-----------------------------|------------|------------|
//! | Yahoo     | Daily OHLCV, metadata       | None       | ✅ Phase 3 |
//! | FRED      | Macro time series           | Free key   | ✅ Phase 3 |
//! | EDGAR     | Fundamentals (IS/BS/CF)     | None       | ✅ Phase 3 |
//! | SimFin    | Fundamentals (IS/BS/CF)     | Free key   | ✅ Phase 3 |
//! | KiBot     | Historical OHLCV + intraday | Paid       | 🔲 Pending |
//! | Tiingo    | OHLCV + news                | Free key   | 🔲 Pending |

mod edgar;
mod fred;
mod kibot;
mod rreichel;
mod simfin;
mod tiingo;
mod traits;
mod yahoo;

pub use self::edgar::EdgarConnector;
pub use self::fred::{
    FetchStats as FredFetchStats, FredConnector, DEFAULT_SERIES as FRED_DEFAULT_SERIES,
};
pub use self::kibot::KibotClient;
pub use self::rreichel::RReichelFeed;
pub use self::simfin::SimFinConnector;
pub use self::tiingo::TiingoFeed;
pub use self::traits::{
    DailyDataProvider, FundamentalsProvider, IntradayDataProvider, NewsProvider, TickDataProvider,
};
pub use self::yahoo::YahooFinanceConnector;
