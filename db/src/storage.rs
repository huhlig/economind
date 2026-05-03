//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

mod duckdb;
mod postgres;
mod postgres_strategy;
mod strategy_traits;
mod traits;

pub use self::duckdb::DuckDatabase;
pub use self::postgres::PostgresStorage;
pub use self::strategy_traits::{
    MacroSeriesPoint, MacroStorage, OpenPosition, PortfolioState, PortfolioStorage,
    StrategyConfigRow, StrategyRunRow, StrategySignalRow, StrategyStorage,
};
pub use self::traits::{
    CandleStorage, MetadataStorage, TickStorage, TickerQuery,
};
