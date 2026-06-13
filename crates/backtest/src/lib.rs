//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind Backtest Engine
//!
//! Historical simulation of strategy runs against DuckDB bar snapshots.
//! Produces performance metrics (Sharpe, drawdown, win rate, etc.) and
//! persists results to PostgreSQL for dashboard display.
//!
//! # Usage
//! ```ignore
//! let result = BacktestRunner::builder()
//!     .strategy_config(config)
//!     .pipeline(runner)
//!     .from_date(NaiveDate::from_ymd_opt(2022, 1, 1).unwrap())
//!     .to_date(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())
//!     .initial_capital(dec!(100_000))
//!     .slippage_bps(5)
//!     .commission_per_trade(dec!(1.00))
//!     .build()
//!     .run(&store)
//!     .await?;
//! ```

pub mod metrics;
pub mod runner;
pub mod simulation;

pub use self::metrics::PerformanceMetrics;
pub use self::runner::{BacktestConfig, BacktestResult, BacktestRunner, BacktestRunnerBuilder};
pub use self::simulation::{SimPortfolio, SimTrade};
