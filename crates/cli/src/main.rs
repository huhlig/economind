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
//! economind universe  list
//! economind universe  add   <SYMBOL> [<SYMBOL>...]
//! economind universe  load  [--file universe.csv]
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
use economind_config::EconomindConfig;

#[derive(Parser)]
#[command(name = "economind")]
#[command(about = "Economind low-frequency trading platform", long_about = None)]
#[command(version)]
struct Cli {
    /// DuckDB file path (defaults to DUCKDB_PATH env var, then economind.toml, then ':memory:').
    #[arg(long, env = "DUCKDB_PATH", global = true)]
    duckdb_path: Option<String>,

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

    /// Manage the tracked instrument universe (add, remove, list symbols).
    Universe(commands::universe::UniverseArgs),

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

    let cfg = EconomindConfig::load()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("economind=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();
    let duckdb_path = cli
        .duckdb_path
        .unwrap_or_else(|| cfg.database.effective_duckdb_path());

    match cli.command {
        Commands::Version => {
            println!("economind {}", env!("CARGO_PKG_VERSION"));
        }

        Commands::Run(args) => {
            commands::run::execute(args, &duckdb_path).await?;
        }

        Commands::Signals(args) => {
            commands::signals::execute(args, &duckdb_path).await?;
        }

        Commands::Universe(args) => {
            commands::universe::execute(args, &duckdb_path).await?;
        }

        Commands::Ingest(args) => {
            commands::ingest::execute(args, &duckdb_path).await?;
        }

        Commands::Backtest(args) => {
            commands::backtest::execute(args, &duckdb_path).await?;
        }

        Commands::Analyze(args) => {
            commands::analyze::execute(args, &duckdb_path).await?;
        }

        Commands::Mcp(args) => {
            let store = economind_db::DataStore::open(&duckdb_path)?;
            let llm = economind_agentic::llm::LlmClientConfig::from_env().map(std::sync::Arc::from);
            economind_agentic::mcp::serve(store, llm, args.port).await?;
        }
    }

    Ok(())
}
