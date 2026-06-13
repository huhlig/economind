//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Reusable Economind command-line action layer.
//!
//! The desktop/Tauri binary calls this library when invoked with a subcommand,
//! so the same executable can serve both dashboard and command-line workflows.

pub mod commands;

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

    /// Provider-specific datafeed fetches.
    Datafeed(commands::datafeed::DatafeedArgs),

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

pub async fn run() -> anyhow::Result<()> {
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

        Commands::Datafeed(args) => {
            commands::datafeed::execute(args, &duckdb_path).await?;
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

pub fn run_blocking() -> anyhow::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(run())
}
