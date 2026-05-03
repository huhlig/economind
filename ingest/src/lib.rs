//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

//! Economind Data Ingest
//!
//! Connectors for all free-tier data sources used in Phase 3.
//!
//! ## Market Data
//! - [x] Yahoo Finance — daily OHLCV, instrument metadata (Phase 3.A)
//!
//! ## Macro Data
//! - [x] FRED — macro time series (Phase 3.B)
//!
//! ## Fundamentals
//! - [x] SEC EDGAR — annual IS/BS/CF via XBRL (Phase 3.C)
//! - [x] SimFin — annual IS/BS/CF via REST API (Phase 3.D)
//!
//! ## Orchestration
//! - [x] DataFeedManager — scheduled ingestion jobs (Phase 3.E)

mod error;
mod httpclient;
mod manager;
mod provider;

pub use self::error::*;
pub use self::httpclient::*;
pub use self::manager::*;
pub use self::provider::*;
