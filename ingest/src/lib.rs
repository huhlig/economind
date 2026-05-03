//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! Economind Data Feeds
//!
//! ## Roadmap
//!
//! ### Market Data
//! - [ ] Polygon
//! - [ ] Tiingo
//! - [ ] Alpha Vantage
//! - [ ] Kibot
//! - [ ] IEX Cloud
//!
//! ### Fundamentals
//! - [ ] SEC EDGAR filings
//! - [ ] Financial Modeling Prep
//! - [ ] Polygon fundamentals
//! - [ ] Alpha Vantage fundamentals
//!
//! ### Risk-Free Rate
//! - [ ] FRED (Federal Reserve Economic Data)
//! - [ ] US Treasury site
//!

mod error;
mod httpclient;
mod provider;

pub use self::error::*;
pub use self::httpclient::*;
pub use self::provider::*;
