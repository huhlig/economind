//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind CLI
//!
//! Thin command-line interface; all business logic lives in other crates.
//!
//! # Subcommands (Phase 2)
//!
//! ```text
//! economind version
//! economind run    --config <uuid>
//! economind signals [--since <date>] [--limit <n>] [--symbol <sym>]
//! ```

mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "economind")]
#[command(about = "Economind low-frequency trading platform", long_about = None)]
#[command(version)]
struct Cli {
    /// PostgreSQL connection URL (defaults to DATABASE_URL env var).
    #[arg(long, env = "DATABASE_URL", global = true)]
    database_url: Option<String>,

    /// DuckDB file path (defaults to DUCKDB_PATH env var, or ':memory:').
    #[arg(long, env = "DUCKDB_PATH", global = true, default_value = ":memory:")]
    duckdb_path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print version information.
    Version,

    /// Run a strategy and emit signals.
    Run(commands::run::RunArgs),

    /// Query and display recent signals.
    Signals(commands::signals::SignalsArgs),
    // TODO: Phase 3 — add `ingest` subcommands
    // TODO: Phase 4 — add `backtest` subcommand
    // TODO: Phase 7 — add `analyze` subcommand
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => {
            println!("economind {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Run(args) => {
            let db_url = cli
                .database_url
                .ok_or_else(|| anyhow::anyhow!(
                    "DATABASE_URL must be set (via --database-url or environment)"
                ))?;
            commands::run::execute(args, &db_url, &cli.duckdb_path).await?;
        }
        Commands::Signals(args) => {
            let db_url = cli
                .database_url
                .ok_or_else(|| anyhow::anyhow!(
                    "DATABASE_URL must be set (via --database-url or environment)"
                ))?;
            commands::signals::execute(args, &db_url, &cli.duckdb_path).await?;
        }
    }

    Ok(())
}
