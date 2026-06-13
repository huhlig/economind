//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Strategy run orchestration (§2.B.2 / §8.A / §8.B).
//!
//! This module provides:
//!
//! - `load_context` — builds a `StrategyContext` from the `DataStore`.
//! - `run_strategy` — executes a `PipelineRunner` (Phase 2 original API, kept for
//!   backwards compatibility).
//! - `run_strategy_multi` — unified entry point that dispatches on
//!   `CompositionMode` across `Pipeline`, `Voting`, and `Ensemble` runners.
//!
//! All three functions persist run metadata and signals to `strategy.runs` /
//! `strategy.signals` via the `DataStore`.
//!
//! Callers (CLI / API layer) are responsible for constructing the concrete
//! runner types with the correct plugin instances for the given `StrategyConfig`.

use crate::config::{CompositionMode, StrategyConfig};
use crate::context::StrategyContext;
use crate::ensemble::EnsembleRunner;
use crate::pipeline::{PipelineRunner, TradeSignal};
use crate::run::{PersistedSignal, RunStatus, StrategyRunResult};
use crate::traits::TradeDirection;
use crate::voting::VotingRunner;
use chrono::Utc;
use economind_core::model::{DailyCandleEntry, Symbol};
use economind_db::storage::{
    MacroStorage, PortfolioStorage, StrategyRunRow, StrategySignalRow, StrategyStorage,
};
use economind_db::{CandleStorage, DataStore, MetadataStorage};
use futures::StreamExt;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

// ── Context loader ────────────────────────────────────────────────────────────

/// Default lookback window for daily bars loaded into StrategyContext.
const DEFAULT_BAR_LOOKBACK_DAYS: u32 = 365;

/// Standard FRED macro series loaded for every strategy run.
const MACRO_SERIES: &[&str] = &[
    "DGS10",    // 10-year treasury yield
    "T10Y2Y",   // 10Y–2Y yield spread (recession indicator)
    "CPIAUCSL", // CPI (inflation)
    "UNRATE",   // Unemployment rate
    "VIXCLS",   // VIX (market fear gauge)
    "M2SL",     // M2 money supply
];

/// Load a `StrategyContext` from the DataStore for the given config.
///
/// Fetches instrument universe, bar history, macro data, and portfolio state.
/// This function is called once per strategy run; all subsequent plugin calls
/// read from the in-memory context only (no further I/O).
pub async fn load_context(
    store: &DataStore,
    config: &StrategyConfig,
    bar_lookback_days: Option<u32>,
) -> Result<StrategyContext, Box<dyn std::error::Error + Send + Sync>> {
    let lookback = bar_lookback_days.unwrap_or(DEFAULT_BAR_LOOKBACK_DAYS);
    let cutoff = {
        let today = chrono::Utc::now().date_naive();
        today - chrono::Duration::days(lookback as i64)
    };
    let end = chrono::Utc::now().date_naive() + chrono::Duration::days(1);

    // 1. Instrument universe — all active symbols.
    let universe: Vec<Symbol> = store.list_tickers().await?.collect().await;

    // 2. Daily bar history per symbol.
    let mut bars: HashMap<Symbol, Vec<DailyCandleEntry>> =
        HashMap::with_capacity(universe.len());
    for sym in &universe {
        let sym_bars: Vec<DailyCandleEntry> = store
            .query_daily_candles(sym, cutoff..end)
            .await?
            .collect()
            .await;
        if !sym_bars.is_empty() {
            bars.insert(sym.clone(), sym_bars);
        }
    }

    // 3. Macro data — latest value per tracked series.
    let macro_points = store
        .get_latest_macro_values(MACRO_SERIES)
        .await
        .unwrap_or_default();
    let mut macro_data: HashMap<String, Decimal> = HashMap::new();
    for point in macro_points {
        if let Some(val) = point.value {
            macro_data.insert(point.series_id, val);
        }
    }

    // 4. Portfolio state.
    let portfolio = store
        .load_portfolio_state()
        .await
        .unwrap_or_else(|_| economind_db::storage::PortfolioState {
            open_positions: vec![],
            portfolio_value: Decimal::ZERO,
            available_cash: Decimal::ZERO,
            current_drawdown: Decimal::ZERO,
        });

    let open_positions: HashMap<Symbol, Decimal> = portfolio
        .open_positions
        .into_iter()
        .map(|p| (p.symbol, p.shares))
        .collect();

    Ok(StrategyContext {
        universe,
        bars,
        fundamentals: HashMap::new(), // populated by Phase 3 (fundamentals ingest)
        macro_data,
        open_positions,
        portfolio_value: portfolio.portfolio_value,
        available_cash: portfolio.available_cash,
        current_drawdown: portfolio.current_drawdown,
        regime: None,
        parameters: config.parameters.clone(),
    })
}

// ── StrategyRunner enum ───────────────────────────────────────────────────────

/// A type-erased strategy runner that wraps all three composition modes.
///
/// Callers construct the appropriate variant and pass it to
/// `run_strategy_multi`, which dispatches and persists results uniformly.
pub enum StrategyRunner {
    /// Single pipeline: Identifier(s) → Timer(s) → Sizer.
    Pipeline(PipelineRunner),
    /// Multi-stack voting: requires a quorum of stacks to agree.
    Voting(VotingRunner),
    /// Multi-stack ensemble: weighted average of stack scores.
    Ensemble(EnsembleRunner),
}

impl StrategyRunner {
    /// Execute the runner against the given context and return raw signals.
    pub async fn run(&self, ctx: &StrategyContext) -> Vec<TradeSignal> {
        match self {
            StrategyRunner::Pipeline(r) => r.run(ctx).await,
            StrategyRunner::Voting(r) => r.run(ctx).await,
            StrategyRunner::Ensemble(r) => r.run(ctx).await,
        }
    }

    /// The composition mode of this runner (for logging/metadata).
    pub fn mode(&self) -> CompositionMode {
        match self {
            StrategyRunner::Pipeline(_) => CompositionMode::Pipeline,
            StrategyRunner::Voting(_) => CompositionMode::Voting,
            StrategyRunner::Ensemble(_) => CompositionMode::Ensemble,
        }
    }
}

// ── run_strategy (legacy / Phase 2 API) ──────────────────────────────────────

/// Execute a full strategy run using a `PipelineRunner` and persist the results.
///
/// This is the Phase 2 pipeline-only API, kept for CLI backwards-compatibility.
/// For Phase 8 multi-mode support use `run_strategy_multi` directly.
///
/// Internally this function shares the same persist-and-report logic as
/// `run_strategy_multi` but takes `&PipelineRunner` directly to avoid moving
/// the runner into a `StrategyRunner` enum variant.
///
/// # Arguments
/// * `config` — The strategy configuration to run.
/// * `runner` — A `PipelineRunner` wired with the correct plugin instances.
/// * `store`  — The DataStore (must have a live PostgreSQL connection).
pub async fn run_strategy(
    config: &StrategyConfig,
    runner: &PipelineRunner,
    store: &DataStore,
) -> StrategyRunResult {
    let run_id = Uuid::new_v4();
    let started_at = Utc::now();

    let run_row = StrategyRunRow {
        id: run_id,
        config_id: config.id,
        started_at,
        completed_at: None,
        status: "running".to_string(),
        signal_count: 0,
        error_message: None,
        config_snapshot_json: serde_json::to_string(config).unwrap_or_default(),
    };
    if let Err(e) = store.insert_strategy_run(&run_row).await {
        return StrategyRunResult {
            run_id,
            config_id: config.id,
            config_snapshot: config.clone(),
            started_at,
            completed_at: Utc::now(),
            status: RunStatus::Failed,
            signals: vec![],
            error_message: Some(format!("Failed to persist run record: {e}")),
        };
    }

    let ctx = match load_context(store, config, None).await {
        Ok(ctx) => ctx,
        Err(e) => {
            let msg = format!("Failed to load StrategyContext: {e}");
            let _ = store
                .complete_strategy_run(&StrategyRunRow {
                    id: run_id,
                    config_id: config.id,
                    started_at,
                    completed_at: Some(Utc::now()),
                    status: "failed".to_string(),
                    signal_count: 0,
                    error_message: Some(msg.clone()),
                    config_snapshot_json: String::new(),
                })
                .await;
            return StrategyRunResult {
                run_id,
                config_id: config.id,
                config_snapshot: config.clone(),
                started_at,
                completed_at: Utc::now(),
                status: RunStatus::Failed,
                signals: vec![],
                error_message: Some(msg),
            };
        }
    };

    persist_and_return(run_id, config, store, runner.run(&ctx).await, started_at).await
}

// ── run_strategy_multi ────────────────────────────────────────────────────────

/// Unified strategy run entry point supporting all three composition modes.
///
/// # Arguments
/// * `config`  — The strategy configuration (provides metadata + config snapshot).
/// * `runner`  — A `StrategyRunner` variant (Pipeline, Voting, or Ensemble).
/// * `store`   — The DataStore for persistence.
///
/// # Returns
/// A `StrategyRunResult` with run metadata and all emitted signals.
pub async fn run_strategy_multi(
    config: &StrategyConfig,
    runner: &StrategyRunner,
    store: &DataStore,
) -> StrategyRunResult {
    let run_id = Uuid::new_v4();
    let started_at = Utc::now();

    // Persist the run record in 'running' state.
    let run_row = StrategyRunRow {
        id: run_id,
        config_id: config.id,
        started_at,
        completed_at: None,
        status: "running".to_string(),
        signal_count: 0,
        error_message: None,
        config_snapshot_json: serde_json::to_string(config).unwrap_or_default(),
    };
    if let Err(e) = store.insert_strategy_run(&run_row).await {
        return StrategyRunResult {
            run_id,
            config_id: config.id,
            config_snapshot: config.clone(),
            started_at,
            completed_at: Utc::now(),
            status: RunStatus::Failed,
            signals: vec![],
            error_message: Some(format!("Failed to persist run record: {e}")),
        };
    }

    // Load context.
    let ctx = match load_context(store, config, None).await {
        Ok(ctx) => ctx,
        Err(e) => {
            let msg = format!("Failed to load StrategyContext: {e}");
            let _ = store
                .complete_strategy_run(&StrategyRunRow {
                    id: run_id,
                    config_id: config.id,
                    started_at,
                    completed_at: Some(Utc::now()),
                    status: "failed".to_string(),
                    signal_count: 0,
                    error_message: Some(msg.clone()),
                    config_snapshot_json: String::new(),
                })
                .await;
            return StrategyRunResult {
                run_id,
                config_id: config.id,
                config_snapshot: config.clone(),
                started_at,
                completed_at: Utc::now(),
                status: RunStatus::Failed,
                signals: vec![],
                error_message: Some(msg),
            };
        }
    };

    // Execute the runner (dispatches on Pipeline / Voting / Ensemble).
    let trade_signals = runner.run(&ctx).await;
    persist_and_return(run_id, config, store, trade_signals, started_at).await
}

// ── Shared persistence helper ─────────────────────────────────────────────────

/// Convert raw `TradeSignal`s into `PersistedSignal`s, write them to the DB,
/// finalise the run record, and return a `StrategyRunResult`.
///
/// Shared by both `run_strategy` and `run_strategy_multi`.
async fn persist_and_return(
    run_id: Uuid,
    config: &StrategyConfig,
    store: &DataStore,
    trade_signals: Vec<TradeSignal>,
    started_at: chrono::DateTime<Utc>,
) -> StrategyRunResult {
    // Convert to PersistedSignals.
    let persisted: Vec<PersistedSignal> = trade_signals
        .iter()
        .map(|ts| PersistedSignal::from_pipeline(run_id, config.id, &ts.timing, &ts.size))
        .collect();

    // Persist signals.
    let signal_rows: Vec<StrategySignalRow> = persisted
        .iter()
        .map(|s| StrategySignalRow {
            id: s.id,
            run_id: s.run_id,
            config_id: s.config_id,
            symbol: s.symbol.as_str().to_string(),
            direction: match s.direction {
                TradeDirection::Long => "long".to_string(),
                TradeDirection::Short => "short".to_string(),
            },
            identifier_score: rust_decimal::Decimal::try_from(s.identifier_score)
                .unwrap_or_default(),
            timing_score: rust_decimal::Decimal::try_from(s.timing_score).unwrap_or_default(),
            position_shares: s.shares,
            position_notional: s.notional,
            portfolio_fraction: s.portfolio_fraction,
            rationale: s.rationale.clone(),
            analysis_brief: s.analysis_brief.clone(),
            emitted_at: s.emitted_at,
        })
        .collect();

    if !signal_rows.is_empty() {
        if let Err(e) = store.insert_strategy_signals(&signal_rows).await {
            eprintln!(
                "Warning: failed to persist {n} signals: {e}",
                n = signal_rows.len()
            );
        }
    }

    let completed_at = Utc::now();

    let _ = store
        .complete_strategy_run(&StrategyRunRow {
            id: run_id,
            config_id: config.id,
            started_at,
            completed_at: Some(completed_at),
            status: "completed".to_string(),
            signal_count: persisted.len() as i32,
            error_message: None,
            config_snapshot_json: serde_json::to_string(config).unwrap_or_default(),
        })
        .await;

    StrategyRunResult {
        run_id,
        config_id: config.id,
        config_snapshot: config.clone(),
        started_at,
        completed_at,
        status: RunStatus::Completed,
        signals: persisted,
        error_message: None,
    }
}
