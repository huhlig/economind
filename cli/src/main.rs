//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind CLI
//!
//! Thin command-line interface; all business logic lives in other crates.
//!
//! # Subcommands
//!
//! ```text
//! economind version
//! economind run       --config <uuid>  [--lookback-days 365]
//! economind signals   [--since <date>] [--limit <n>] [--symbol <sym>] [--config <uuid>]
//! economind ingest    bars          [--since <date>] [--concurrency <n>]
//! economind ingest    macro         [--since <date>] [--series <ids>]
//! economind ingest    fundamentals  [--edgar-only] [--simfin-only]
//! economind backtest  run           --strategy <uuid> --from <date> --to <date>
//! economind backtest  list          [--strategy <uuid>] [--limit <n>]
//! economind analyze   signal        <signal-uuid>
//! economind analyze   instrument    <symbol>
//! economind analyze   macro
//! economind mcp                     [--port 8081]
//! ```

mod commands;

use clap::{Args, Parser, Subcommand};

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

    /// On-demand data ingestion (bars, macro, fundamentals).
    Ingest(commands::ingest::IngestArgs),

    /// Run a historical backtest or list past backtest runs.
    Backtest(commands::backtest::BacktestArgs),

    /// LLM-powered analysis of signals, instruments, and macro environment.
    Analyze(commands::analyze::AnalyzeArgs),

    /// Start the MCP server (Model Context Protocol endpoint for Claude integration).
    Mcp(McpArgs),
}

#[derive(Args)]
struct McpArgs {
    /// Port to bind the MCP server on (default: 8081).
    #[arg(long, default_value = "8081")]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("economind=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Version => {
            println!("economind {}", env!("CARGO_PKG_VERSION"));
        }

        Commands::Run(args) => {
            let db_url = require_db_url(cli.database_url)?;
            commands::run::execute(args, &db_url, &cli.duckdb_path).await?;
        }

        Commands::Signals(args) => {
            let db_url = require_db_url(cli.database_url)?;
            commands::signals::execute(args, &db_url, &cli.duckdb_path).await?;
        }

        Commands::Ingest(args) => {
            let db_url = require_db_url(cli.database_url)?;
            commands::ingest::execute(args, &db_url, &cli.duckdb_path).await?;
        }

        Commands::Backtest(args) => {
            let db_url = require_db_url(cli.database_url)?;
            commands::backtest::execute(args, &db_url, &cli.duckdb_path).await?;
        }

        Commands::Analyze(args) => {
            let db_url = require_db_url(cli.database_url)?;
            commands::analyze::execute(args, &db_url, &cli.duckdb_path).await?;
        }

        Commands::Mcp(args) => {
            let db_url = require_db_url(cli.database_url)?;
            let store = economind_db::DataStore::connect(&db_url, &cli.duckdb_path).await?;
            let llm = economind_agentic::llm::LlmClientConfig::from_env()
                .map(std::sync::Arc::from);
            economind_agentic::mcp::serve(store, llm, args.port).await?;
        }
    }

    Ok(())
}

fn require_db_url(db_url: Option<String>) -> anyhow::Result<String> {
    db_url.ok_or_else(|| {
        anyhow::anyhow!("DATABASE_URL must be set (via --database-url or environment)")
    })
}
