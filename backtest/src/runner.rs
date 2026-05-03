//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `BacktestRunner` — the top-level simulation loop (§4.A.2).
//!
//! Steps through historical dates one bar at a time, builds a `StrategyContext`
//! from data up to (but not including) the current date (no lookahead), runs
//! the `PipelineRunner`, simulates order fills, and records all activity.
//! At the end, computes performance metrics and persists everything to PostgreSQL.

use crate::metrics::{EquityPoint, PerformanceMetrics};
use crate::simulation::SimPortfolio;
use chrono::NaiveDate;
use economind_core::model::{DailyCandleEntry, Symbol};
use economind_db::{
    BacktestRunRow, BacktestStorage, BacktestTradeRow, CandleStorage,
    EquityCurvePoint, MacroStorage, MetadataStorage, DataStore,
};
use economind_strategy::{
    config::StrategyConfig,
    context::StrategyContext,
    pipeline::PipelineRunner,
    traits::TradeDirection,
};
use futures::StreamExt;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

// ── BacktestConfig ────────────────────────────────────────────────────────────

/// Parameters controlling a single backtest run.
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub strategy_config: StrategyConfig,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub initial_capital: Decimal,
    /// Slippage applied to fills, in basis points (e.g. 5 = 0.05%).
    pub slippage_bps: u32,
    /// Flat commission per trade side (entry or exit).
    pub commission_per_trade: Decimal,
    /// Maximum days to hold a position before forced exit.
    pub max_hold_days: u32,
    /// Minimum timing score required to enter a position (0.0–1.0).
    pub entry_score_threshold: f64,
    /// Maximum fraction of portfolio per single position (e.g. 0.05 = 5%).
    pub max_position_pct: Decimal,
    /// Daily risk-free rate used in Sharpe/Sortino computation.
    /// If `None`, defaults to 0 (no risk-free adjustment).
    pub risk_free_daily: Option<Decimal>,
}

// ── BacktestResult ────────────────────────────────────────────────────────────

/// The full outcome of a completed backtest run.
#[derive(Debug)]
pub struct BacktestResult {
    pub run_id: Uuid,
    pub config: BacktestConfig,
    pub metrics: PerformanceMetrics,
}

// ── BacktestRunner ────────────────────────────────────────────────────────────

/// Runs a historical backtest for a given strategy pipeline.
pub struct BacktestRunner {
    config: BacktestConfig,
    pipeline: PipelineRunner,
}

impl BacktestRunner {
    pub fn builder() -> BacktestRunnerBuilder {
        BacktestRunnerBuilder::default()
    }

    /// Execute the backtest.
    ///
    /// Requires `store` to have a PostgreSQL connection (for result persistence)
    /// and populated bar data in DuckDB.
    pub async fn run(self, store: &DataStore) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let run_id = Uuid::new_v4();
        let started_at = chrono::Utc::now();

        // Destructure self immediately so we have owned values throughout.
        let BacktestRunner { config, pipeline } = self;

        // Snapshot immutable values needed for DB rows.
        let config_id = config.strategy_config.id;
        let config_snapshot_json = serde_json::to_string(&config.strategy_config)
            .unwrap_or_default();
        let from_date = config.from_date;
        let to_date = config.to_date;
        let initial_capital = config.initial_capital;

        // Persist run record (status = running).
        let run_row = BacktestRunRow {
            id: run_id,
            config_id,
            config_snapshot_json: config_snapshot_json.clone(),
            from_date,
            to_date,
            initial_capital,
            final_capital: None,
            cagr: None,
            sharpe_ratio: None,
            sortino_ratio: None,
            max_drawdown: None,
            max_drawdown_days: None,
            win_rate: None,
            profit_factor: None,
            expectancy: None,
            total_trades: None,
            avg_hold_days: None,
            status: "running".to_string(),
            started_at,
            completed_at: None,
            error_message: None,
        };
        store.insert_backtest_run(&run_row).await?;

        // Run the simulation; on error, mark as failed and return.
        match simulate(run_id, &config, pipeline, store).await {
            Ok(metrics) => {
                let completed_at = chrono::Utc::now();

                // Persist equity curve.
                let curve_rows: Vec<EquityCurvePoint> = metrics
                    .equity_curve
                    .iter()
                    .map(|p| EquityCurvePoint {
                        run_id,
                        date: p.date,
                        portfolio_value: p.value,
                        cash: p.cash,
                        drawdown: p.drawdown,
                    })
                    .collect();
                if !curve_rows.is_empty() {
                    store.insert_equity_curve(&curve_rows).await?;
                }

                // Complete the run row with all metrics.
                let completed_row = BacktestRunRow {
                    id: run_id,
                    config_id,
                    config_snapshot_json,
                    from_date,
                    to_date,
                    initial_capital,
                    final_capital: Some(metrics.final_capital),
                    cagr: Some(metrics.cagr),
                    sharpe_ratio: Some(metrics.sharpe_ratio),
                    sortino_ratio: Some(metrics.sortino_ratio),
                    max_drawdown: Some(metrics.max_drawdown),
                    max_drawdown_days: Some(metrics.max_drawdown_days),
                    win_rate: Some(metrics.win_rate),
                    profit_factor: Some(metrics.profit_factor),
                    expectancy: Some(metrics.expectancy),
                    total_trades: Some(metrics.total_trades),
                    avg_hold_days: Some(metrics.avg_hold_days),
                    status: "completed".to_string(),
                    started_at,
                    completed_at: Some(completed_at),
                    error_message: None,
                };
                store.complete_backtest_run(&completed_row).await?;

                Ok(BacktestResult { run_id, config, metrics })
            }
            Err(e) => {
                let msg = e.to_string();
                let failed_row = BacktestRunRow {
                    id: run_id,
                    config_id,
                    config_snapshot_json: String::new(),
                    from_date,
                    to_date,
                    initial_capital,
                    final_capital: None,
                    cagr: None,
                    sharpe_ratio: None,
                    sortino_ratio: None,
                    max_drawdown: None,
                    max_drawdown_days: None,
                    win_rate: None,
                    profit_factor: None,
                    expectancy: None,
                    total_trades: None,
                    avg_hold_days: None,
                    status: "failed".to_string(),
                    started_at,
                    completed_at: Some(chrono::Utc::now()),
                    error_message: Some(msg),
                };
                let _ = store.complete_backtest_run(&failed_row).await;
                Err(e)
            }
        }
    }

}

// ── Core simulation loop (free function) ─────────────────────────────────────

async fn simulate(
    run_id: Uuid,
    config: &BacktestConfig,
    pipeline: PipelineRunner,
    store: &DataStore,
) -> Result<PerformanceMetrics, Box<dyn std::error::Error + Send + Sync>> {
    // Load the full instrument universe.
    let universe: Vec<Symbol> = store.list_tickers().await?.collect().await;

    // Pre-load all daily bars for every symbol across the full backtest window.
    let all_bars = load_all_bars(store, &universe, config.from_date, config.to_date).await?;

    // Pre-load macro series.
    let macro_series = load_macro_series(store, config.from_date, config.to_date).await?;

    // Build sorted list of trading dates within the backtest window.
    let trading_dates = collect_trading_dates(&all_bars, config.from_date, config.to_date);

    if trading_dates.is_empty() {
        return Err("No trading dates found in the backtest window — ensure bars are ingested".into());
    }

    // Initialise simulated portfolio.
    let mut portfolio = SimPortfolio::new(
        config.initial_capital,
        config.commission_per_trade,
        config.slippage_bps,
        config.max_position_pct,
    );

    // daily_values: date → (portfolio_value, cash)
    let mut daily_values: BTreeMap<NaiveDate, (Decimal, Decimal)> = BTreeMap::new();

    for (idx, &date) in trading_dates.iter().enumerate() {
        // 1. Build StrategyContext from bars strictly before `date` (no lookahead).
        let ctx = build_historical_context(
            &all_bars,
            &macro_series,
            &portfolio,
            &config.strategy_config,
            date,
        );

        // 2. Latest prices at today's open (for fills and MTM).
        let latest_prices = get_prices_at_date(&all_bars, date);

        // 3. Exit positions that have exceeded max_hold_days.
        let stale_symbols: Vec<Symbol> = portfolio
            .open_positions
            .values()
            .filter(|p| (date - p.entry_date).num_days() as u32 >= config.max_hold_days)
            .map(|p| p.symbol.clone())
            .collect();
        for sym in &stale_symbols {
            if let Some(&open_price) = latest_prices.get(sym) {
                portfolio.exit_position(sym, date, open_price);
            }
        }

        // 4. Run the strategy pipeline for new entry signals.
        let signals = pipeline.run(&ctx).await;

        // 5. Enter new positions for qualifying signals.
        for signal in &signals {
            let sym = &signal.timing.candidate.symbol;
            if signal.timing.score < config.entry_score_threshold {
                continue;
            }
            if signal.timing.direction != TradeDirection::Long {
                continue; // long-only; short support planned for Phase 8
            }
            if let Some(&open_price) = latest_prices.get(sym) {
                let portfolio_val = portfolio.portfolio_value(&latest_prices);
                let max_notional = portfolio.cash
                    .min(portfolio_val * config.max_position_pct);
                if open_price.is_zero() || max_notional <= Decimal::ZERO {
                    continue;
                }
                let shares = (max_notional / open_price).min(signal.size.shares);
                if shares > Decimal::ZERO {
                    portfolio.enter_long(sym.clone(), date, open_price, shares);
                }
            }
        }

        // 6. Record daily equity.
        let pv = portfolio.portfolio_value(&latest_prices);
        daily_values.insert(date, (pv, portfolio.cash));

        // 7. On the last day, close all remaining open positions.
        if idx == trading_dates.len() - 1 {
            portfolio.close_all(date, &latest_prices);
            let final_pv = portfolio.portfolio_value(&latest_prices);
            daily_values.insert(date, (final_pv, portfolio.cash));
        }
    }

    // Persist trades to DB.
    let trade_rows: Vec<BacktestTradeRow> = portfolio
        .closed_trades
        .iter()
        .map(|t| BacktestTradeRow {
            id: t.id,
            run_id,
            symbol: t.symbol.as_str().to_string(),
            direction: t.direction.clone(),
            entry_date: t.entry_date,
            entry_price: t.entry_price,
            exit_date: Some(t.exit_date),
            exit_price: Some(t.exit_price),
            shares: t.shares,
            gross_pnl: Some(t.gross_pnl),
            commission: t.commission,
            net_pnl: Some(t.net_pnl),
            hold_days: Some(t.hold_days),
        })
        .collect();

    if !trade_rows.is_empty() {
        store.insert_backtest_trades(&trade_rows).await?;
    }

    // Compute all metrics.
    let rf_daily = config.risk_free_daily.unwrap_or(Decimal::ZERO);
    let metrics = PerformanceMetrics::compute(
        &daily_values,
        &portfolio.closed_trades,
        config.initial_capital,
        rf_daily,
    );

    Ok(metrics)
}

// ── Data loaders ──────────────────────────────────────────────────────────────

/// Load all daily bars for every symbol across the backtest window.
async fn load_all_bars(
    store: &DataStore,
    universe: &[Symbol],
    from: NaiveDate,
    to: NaiveDate,
) -> Result<HashMap<Symbol, Vec<DailyCandleEntry>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut all: HashMap<Symbol, Vec<DailyCandleEntry>> = HashMap::new();
    // Load a few extra days before `from` so the first context has lookback data.
    let load_from = from - chrono::Duration::days(400);
    let load_to = to + chrono::Duration::days(1);

    for sym in universe {
        let bars: Vec<DailyCandleEntry> = store
            .query_daily_candles(sym, load_from..load_to)
            .await?
            .collect()
            .await;
        if !bars.is_empty() {
            all.insert(sym.clone(), bars);
        }
    }
    Ok(all)
}

/// Load macro series for the entire backtest window.
/// Returns a map: series_id → Vec<(date, value)> sorted by date.
async fn load_macro_series(
    store: &DataStore,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<HashMap<String, Vec<(NaiveDate, Decimal)>>, Box<dyn std::error::Error + Send + Sync>> {
    const SERIES: &[&str] = &["DGS10", "T10Y2Y", "CPIAUCSL", "UNRATE", "VIXCLS", "M2SL"];
    let load_from = from - chrono::Duration::days(60); // allow lookback for macro
    let mut result: HashMap<String, Vec<(NaiveDate, Decimal)>> = HashMap::new();

    for series_id in SERIES {
        match store.query_macro_series(series_id, load_from..to).await {
            Ok(points) => {
                let vals: Vec<(NaiveDate, Decimal)> = points
                    .into_iter()
                    .filter_map(|p| p.value.map(|v| (p.date, v)))
                    .collect();
                result.insert(series_id.to_string(), vals);
            }
            Err(_) => {} // macro data is optional
        }
    }
    Ok(result)
}

/// Collect all trading dates within [from, to] that appear in the bar data.
fn collect_trading_dates(
    all_bars: &HashMap<Symbol, Vec<DailyCandleEntry>>,
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<NaiveDate> {
    let mut dates: std::collections::BTreeSet<NaiveDate> = std::collections::BTreeSet::new();
    for bars in all_bars.values() {
        for bar in bars {
            if bar.date >= from && bar.date <= to {
                dates.insert(bar.date);
            }
        }
    }
    dates.into_iter().collect()
}

/// Get the open price for every symbol that has a bar on `date`.
fn get_prices_at_date(
    all_bars: &HashMap<Symbol, Vec<DailyCandleEntry>>,
    date: NaiveDate,
) -> HashMap<Symbol, Decimal> {
    let mut prices = HashMap::new();
    for (sym, bars) in all_bars {
        if let Some(bar) = bars.iter().find(|b| b.date == date) {
            prices.insert(sym.clone(), bar.open);
        }
    }
    prices
}

/// Build a `StrategyContext` using only data strictly before `date` (no lookahead).
fn build_historical_context(
    all_bars: &HashMap<Symbol, Vec<DailyCandleEntry>>,
    macro_series: &HashMap<String, Vec<(NaiveDate, Decimal)>>,
    portfolio: &SimPortfolio,
    config: &StrategyConfig,
    date: NaiveDate,
) -> StrategyContext {
    // Only include bars strictly before `date`.
    let bars: HashMap<Symbol, Vec<DailyCandleEntry>> = all_bars
        .iter()
        .filter_map(|(sym, v)| {
            let filtered: Vec<DailyCandleEntry> =
                v.iter().filter(|b| b.date < date).cloned().collect();
            if filtered.is_empty() {
                None
            } else {
                Some((sym.clone(), filtered))
            }
        })
        .collect();

    let universe: Vec<Symbol> = bars.keys().cloned().collect();

    // Latest macro value at or before `date`.
    let mut macro_data: HashMap<String, Decimal> = HashMap::new();
    for (series_id, points) in macro_series {
        if let Some((_, val)) = points.iter().rev().find(|(d, _)| *d <= date) {
            macro_data.insert(series_id.clone(), *val);
        }
    }

    // Reconstruct open positions from simulated portfolio.
    let open_positions: HashMap<Symbol, Decimal> = portfolio
        .open_positions
        .values()
        .map(|p| (p.symbol.clone(), p.shares))
        .collect();

    // Approximate portfolio value using last known close prices.
    let position_value: Decimal = portfolio
        .open_positions
        .values()
        .map(|p| {
            let last_close = bars
                .get(&p.symbol)
                .and_then(|v| v.last())
                .map(|b| b.close)
                .unwrap_or(p.entry_price);
            last_close * p.shares
        })
        .sum();
    let portfolio_value = portfolio.cash + position_value;

    StrategyContext {
        universe,
        bars,
        fundamentals: HashMap::new(), // Phase 5+ enhancement
        macro_data,
        open_positions,
        portfolio_value,
        available_cash: portfolio.cash,
        current_drawdown: Decimal::ZERO, // computed externally
        regime: None,
        parameters: config.parameters.clone(),
    }
}

// ── BacktestRunnerBuilder ─────────────────────────────────────────────────────

#[derive(Default)]
pub struct BacktestRunnerBuilder {
    strategy_config: Option<StrategyConfig>,
    pipeline: Option<PipelineRunner>,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    initial_capital: Option<Decimal>,
    slippage_bps: Option<u32>,
    commission_per_trade: Option<Decimal>,
    max_hold_days: Option<u32>,
    entry_score_threshold: Option<f64>,
    max_position_pct: Option<Decimal>,
    risk_free_daily: Option<Decimal>,
}

impl BacktestRunnerBuilder {
    pub fn strategy_config(mut self, c: StrategyConfig) -> Self {
        self.strategy_config = Some(c);
        self
    }
    pub fn pipeline(mut self, p: PipelineRunner) -> Self {
        self.pipeline = Some(p);
        self
    }
    pub fn from_date(mut self, d: NaiveDate) -> Self {
        self.from_date = Some(d);
        self
    }
    pub fn to_date(mut self, d: NaiveDate) -> Self {
        self.to_date = Some(d);
        self
    }
    pub fn initial_capital(mut self, c: Decimal) -> Self {
        self.initial_capital = Some(c);
        self
    }
    pub fn slippage_bps(mut self, bps: u32) -> Self {
        self.slippage_bps = Some(bps);
        self
    }
    pub fn commission_per_trade(mut self, c: Decimal) -> Self {
        self.commission_per_trade = Some(c);
        self
    }
    pub fn max_hold_days(mut self, d: u32) -> Self {
        self.max_hold_days = Some(d);
        self
    }
    pub fn entry_score_threshold(mut self, t: f64) -> Self {
        self.entry_score_threshold = Some(t);
        self
    }
    pub fn max_position_pct(mut self, p: Decimal) -> Self {
        self.max_position_pct = Some(p);
        self
    }
    pub fn risk_free_daily(mut self, r: Decimal) -> Self {
        self.risk_free_daily = Some(r);
        self
    }

    /// Build the `BacktestRunner`.
    ///
    /// # Panics
    /// Panics if `strategy_config`, `pipeline`, `from_date`, or `to_date` were not set.
    pub fn build(self) -> BacktestRunner {
        BacktestRunner {
            config: BacktestConfig {
                strategy_config: self.strategy_config.expect("strategy_config is required"),
                from_date: self.from_date.expect("from_date is required"),
                to_date: self.to_date.expect("to_date is required"),
                initial_capital: self.initial_capital.unwrap_or_else(|| Decimal::from(100_000u32)),
                slippage_bps: self.slippage_bps.unwrap_or(5),
                commission_per_trade: self.commission_per_trade.unwrap_or(Decimal::ONE),
                max_hold_days: self.max_hold_days.unwrap_or(30),
                entry_score_threshold: self.entry_score_threshold.unwrap_or(0.5),
                max_position_pct: self.max_position_pct
                    .unwrap_or_else(|| Decimal::new(5, 2)), // 5%
                risk_free_daily: self.risk_free_daily,
            },
            pipeline: self.pipeline.expect("pipeline is required"),
        }
    }
}
