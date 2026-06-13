//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `economind backtest run` — run a historical backtest and print a summary.
//!
//! # Usage
//! ```text
//! economind backtest run --strategy <uuid> --from <YYYY-MM-DD> --to <YYYY-MM-DD>
//!                        [--initial-capital <f64>]
//!                        [--slippage-bps <u32>]
//!                        [--commission <f64>]
//!                        [--max-hold-days <u32>]
//! ```

use anyhow::{bail, Context};
use chrono::NaiveDate;
use clap::{Args, Subcommand};
use economind_backtest::BacktestRunner;
use economind_db::{DataStore, StrategyStorage};
use rust_decimal::Decimal;
use uuid::Uuid;

// ── CLI types ─────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct BacktestArgs {
    #[command(subcommand)]
    pub command: BacktestCommands,
}

#[derive(Subcommand)]
pub enum BacktestCommands {
    /// Run a backtest for a strategy config over a historical date range.
    Run(BacktestRunArgs),
    /// List recent backtest runs.
    List(BacktestListArgs),
}

#[derive(Args)]
pub struct BacktestRunArgs {
    /// UUID of the strategy config to backtest.
    #[arg(long)]
    pub strategy: Uuid,

    /// Start date (inclusive), e.g. 2022-01-01.
    #[arg(long)]
    pub from: NaiveDate,

    /// End date (inclusive), e.g. 2024-01-01.
    #[arg(long)]
    pub to: NaiveDate,

    /// Starting capital in dollars (default: 100000).
    #[arg(long, default_value = "100000")]
    pub initial_capital: f64,

    /// Slippage in basis points applied to fills (default: 5).
    #[arg(long, default_value = "5")]
    pub slippage_bps: u32,

    /// Flat commission per trade side in dollars (default: 1.00).
    #[arg(long, default_value = "1.00")]
    pub commission: f64,

    /// Maximum holding period in days before forced exit (default: 30).
    #[arg(long, default_value = "30")]
    pub max_hold_days: u32,
}

#[derive(Args)]
pub struct BacktestListArgs {
    /// Filter by strategy config UUID (optional).
    #[arg(long)]
    pub strategy: Option<Uuid>,

    /// Maximum number of runs to show (default: 20).
    #[arg(long, default_value = "20")]
    pub limit: u32,
}

// ── Executor ──────────────────────────────────────────────────────────────────

pub async fn execute(args: BacktestArgs, duckdb_path: &str) -> anyhow::Result<()> {
    match args.command {
        BacktestCommands::Run(run_args) => execute_run(run_args, duckdb_path).await,
        BacktestCommands::List(list_args) => execute_list(list_args, duckdb_path).await,
    }
}

async fn execute_run(args: BacktestRunArgs, duckdb_path: &str) -> anyhow::Result<()> {
    let store = DataStore::open(duckdb_path).context("Failed to open DataStore")?;

    // Load strategy config.
    let config_row = store
        .get_strategy_config(args.strategy)
        .await
        .context("Failed to query strategy config")?
        .with_context(|| format!("Strategy config {} not found", args.strategy))?;

    if !config_row.enabled {
        bail!("Strategy config {} is disabled", args.strategy);
    }

    let config = economind_api::pipeline_factory::strategy_config_from_row(config_row)
        .context("Failed to reconstruct strategy config")?;

    // Preload hot tables into memory for fast strategy reads.
    let lookback_days = ((args.to - args.from).num_days() + 400) as u32;
    store
        .preload(lookback_days)
        .await
        .context("Failed to preload hot data into memory")?;

    let pipeline = economind_api::pipeline_factory::build_pipeline(&config)
        .context("Failed to build strategy pipeline")?;

    let initial_capital =
        Decimal::try_from(args.initial_capital).context("Invalid initial_capital value")?;
    let commission = Decimal::try_from(args.commission).context("Invalid commission value")?;

    println!(
        "Backtesting strategy '{}' ({}) from {} to {}",
        config.name, args.strategy, args.from, args.to
    );
    println!(
        "Capital: ${:.2}  |  Slippage: {}bps  |  Commission: ${:.2}/side  |  Max hold: {} days",
        initial_capital, args.slippage_bps, commission, args.max_hold_days
    );
    println!("Running simulation…\n");

    let runner = BacktestRunner::builder()
        .strategy_config(config)
        .pipeline(pipeline)
        .from_date(args.from)
        .to_date(args.to)
        .initial_capital(initial_capital)
        .slippage_bps(args.slippage_bps)
        .commission_per_trade(commission)
        .max_hold_days(args.max_hold_days)
        .build();

    let result = runner
        .run(&store)
        .await
        .context("Backtest simulation failed")?;

    print_summary(&result);
    Ok(())
}

async fn execute_list(args: BacktestListArgs, duckdb_path: &str) -> anyhow::Result<()> {
    use economind_db::BacktestStorage;

    let store = DataStore::open(duckdb_path).context("Failed to open DataStore")?;

    let runs = store
        .list_backtest_runs(args.strategy, Some(args.limit))
        .await
        .context("Failed to list backtest runs")?;

    if runs.is_empty() {
        println!("No backtest runs found.");
        return Ok(());
    }

    println!(
        "\n{:<38} {:<36} {:<12} {:<12} {:<10} {:<10} {:<8} MaxDD",
        "Run ID", "Config ID", "From", "To", "Status", "CAGR", "Sharpe"
    );
    println!("{}", "─".repeat(140));

    for run in &runs {
        println!(
            "{:<38} {:<36} {:<12} {:<12} {:<10} {:<10} {:<8} {}",
            run.id,
            run.config_id,
            run.from_date,
            run.to_date,
            run.status,
            run.cagr
                .map(|v| format!("{:.2}%", v * Decimal::from(100u32)))
                .unwrap_or_else(|| "—".to_string()),
            run.sharpe_ratio
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "—".to_string()),
            run.max_drawdown
                .map(|v| format!("{:.1}%", v * Decimal::from(100u32)))
                .unwrap_or_else(|| "—".to_string()),
        );
    }

    Ok(())
}

// ── Summary printer ───────────────────────────────────────────────────────────

fn print_summary(result: &economind_backtest::BacktestResult) {
    let m = &result.metrics;

    println!("╔══════════════════════════════════════════════════╗");
    println!("║           BACKTEST RESULTS SUMMARY               ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  Run ID : {:<39}║", result.run_id);
    println!(
        "║  Period : {} → {}               ║",
        result.config.from_date, result.config.to_date
    );
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  RETURN METRICS                                  ║");
    println!("║  Initial Capital : ${:<29.2}║", m.initial_capital);
    println!("║  Final Capital   : ${:<29.2}║", m.final_capital);
    let total_return = if m.initial_capital.is_zero() {
        Decimal::ZERO
    } else {
        (m.final_capital - m.initial_capital) / m.initial_capital * Decimal::from(100u32)
    };
    println!("║  Total Return    : {:<28.2}% ║", total_return);
    println!(
        "║  CAGR            : {:<28.2}% ║",
        m.cagr * Decimal::from(100u32)
    );
    println!("║  Sharpe Ratio    : {:<30.3}║", m.sharpe_ratio);
    println!("║  Sortino Ratio   : {:<30.3}║", m.sortino_ratio);
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  DRAWDOWN                                        ║");
    println!(
        "║  Max Drawdown    : {:<28.2}% ║",
        m.max_drawdown * Decimal::from(100u32)
    );
    println!("║  Max DD Duration : {:<26} days ║", m.max_drawdown_days);
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  TRADE METRICS                                   ║");
    println!("║  Total Trades    : {:<30}║", m.total_trades);
    println!(
        "║  Win Rate        : {:<28.2}% ║",
        m.win_rate * Decimal::from(100u32)
    );
    println!("║  Avg Win         : ${:<29.2}║", m.avg_win);
    println!("║  Avg Loss        : ${:<29.2}║", m.avg_loss);
    println!("║  Profit Factor   : {:<30.3}║", m.profit_factor);
    println!("║  Expectancy      : ${:<29.2}║", m.expectancy);
    println!("║  Avg Hold Days   : {:<30.1}║", m.avg_hold_days);
    println!("║  Largest Win     : ${:<29.2}║", m.largest_win);
    println!("║  Largest Loss    : ${:<29.2}║", m.largest_loss);
    println!("╚══════════════════════════════════════════════════╝");
}
