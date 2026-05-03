//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use clap::{Parser, Subcommand};
use economind_ingest::{RReichelFeed, TiingoFeed};
use std::env;
use economind_db::storage::DuckDatabase;

#[derive(Parser)]
#[command(name = "datafeed")]
#[command(about = "Datafeed ingestor for Economind", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pull data from RReichel (US stock symbols)
    Rreichel,
    /// Pull data from Tiingo
    Tiingo {
        #[command(subcommand)]
        action: TiingoAction,
    },
}

#[derive(Subcommand)]
enum TiingoAction {
    /// Fetch ticker metadata
    Metadata {
        /// The ticker symbol to fetch
        ticker: String,
    },
    /// Fetch ticker daily prices
    Prices {
        /// The ticker symbol to fetch
        ticker: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();


    let db_path = env::var("DUCKDB_PATH").unwrap_or_else(|_| "economind.duckdb".to_string());
    let data_manager = DuckDatabase::open(db_path).expect("Failed to open DuckDB");

    match cli.command {
        Commands::Rreichel => {
            let feed = RReichelFeed::new(data_manager);
            feed.upsert_tickers().await?;
            println!("RReichel datafeed completed successfully.");
        }
        Commands::Tiingo { action } => {
            let api_key = env::var("TIINGO_API_KEY").expect("TIINGO_API_KEY must be set");
            let feed = TiingoFeed::new(data_manager, api_key);
            match action {
                TiingoAction::Metadata { ticker } => {
                    feed.fetch_ticker_metadata(&ticker).await?;
                    println!("Tiingo metadata for {} fetched successfully.", ticker);
                }
                TiingoAction::Prices { ticker } => {
                    feed.fetch_ticker_prices(&ticker, None).await?;
                    println!("Tiingo prices for {} fetched successfully.", ticker);
                }
            }
        }
    }

    Ok(())
}
