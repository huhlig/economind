//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

mod duckdb;
mod strategy_traits;
mod traits;

pub use self::duckdb::DuckDatabase;
pub use self::strategy_traits::{
    BacktestRunRow, BacktestStorage, BacktestTradeRow, ChatMessageRow, ChatSessionRow,
    ChatStorage, EquityCurvePoint, MacroSeriesPoint, MacroStorage, OpenPosition, PortfolioState,
    PortfolioStorage, StrategyConfigRow, StrategyRunRow, StrategySignalRow, StrategyStorage,
};
pub use self::traits::{CandleStorage, MetadataStorage, TickStorage, TickerQuery};
