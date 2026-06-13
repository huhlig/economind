//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `economind datafeed` — provider-specific datafeed fetch commands.

use anyhow::Context;
use clap::{Args, Subcommand};
use economind_db::DuckDatabase;
use economind_ingest::{RReichelFeed, TiingoFeed};

#[derive(Args)]
pub struct DatafeedArgs {
    #[command(subcommand)]
    pub command: DatafeedCommand,
}

#[derive(Subcommand)]
pub enum DatafeedCommand {
    /// Pull ticker metadata from RReichel.
    Rreichel,

    /// Pull data from Tiingo.
    Tiingo {
        #[command(subcommand)]
        action: TiingoAction,
    },
}

#[derive(Subcommand)]
pub enum TiingoAction {
    /// Fetch ticker metadata.
    Metadata {
        /// The ticker symbol to fetch.
        ticker: String,
    },

    /// Fetch daily prices.
    Prices {
        /// The ticker symbol to fetch.
        ticker: String,
    },
}

pub async fn execute(args: DatafeedArgs, duckdb_path: &str) -> anyhow::Result<()> {
    let db = DuckDatabase::open(duckdb_path).context("Failed to open DuckDB")?;

    match args.command {
        DatafeedCommand::Rreichel => {
            let feed = RReichelFeed::new(db);
            feed.upsert_tickers()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!("RReichel datafeed fetch completed.");
        }
        DatafeedCommand::Tiingo { action } => {
            let api_key = std::env::var("TIINGO_API_KEY")
                .context("TIINGO_API_KEY must be set for Tiingo datafeed fetches")?;
            let feed = TiingoFeed::new(db, api_key);
            match action {
                TiingoAction::Metadata { ticker } => {
                    feed.fetch_ticker_metadata(&ticker)
                        .await
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    println!("Tiingo metadata fetch completed for {ticker}.");
                }
                TiingoAction::Prices { ticker } => {
                    feed.fetch_ticker_prices(&ticker, None)
                        .await
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    println!("Tiingo price fetch completed for {ticker}.");
                }
            }
        }
    }

    Ok(())
}
