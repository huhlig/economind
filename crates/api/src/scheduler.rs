//! Background scheduler — runs nightly ingestion, strategy, and execution jobs.
//!
//! All times are UTC.  Configure via `economind.toml` `[schedule]` section or env vars:
//!
//! | Env var                           | Default | Description                          |
//! |-----------------------------------|---------|--------------------------------------|
//! | `ECONOMIND_SCHED_BARS_HH_MM`      | `22:00` | Daily bar ingestion (5 PM ET)        |
//! | `ECONOMIND_SCHED_MACRO_HH_MM`     | `23:00` | Daily macro refresh (6 PM ET)        |
//! | `ECONOMIND_SCHED_FUND_HH_MM`      | `23:00` | Weekly fundamentals (6 PM ET Sunday) |
//! | `ECONOMIND_SCHED_STRATEGY_HH_MM`  | `23:30` | Daily strategy run (6:30 PM ET)      |
//! | `ECONOMIND_SCHED_BARS_LOOKBACK`   | `5`     | Days of bars to backfill on each run |

use chrono::{Datelike, NaiveTime, Timelike, Utc, Weekday};
use economind_broker::{AlpacaConnector, BrokerConnector, OrderRequest, OrderSide};
use economind_config::{EconomindConfig, NotificationsConfig};
use economind_db::{DataStore, PortfolioStorage, StrategyStorage};
use economind_ingest::{
    DataFeedManager, DataFeedManagerConfig, EdgarConnector, FredConnector, SimFinConnector,
    YahooFinanceConnector,
};
use economind_strategy::config::{CompositionMode, ExecutionMode, PluginSpec, StrategyConfig};
use economind_strategy::run::PersistedSignal;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep_until, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{events::ServerEvent, pipeline_factory, state::AppState};

// ── Public entry point ────────────────────────────────────────────────────────

/// Spawn all scheduler tasks as detached background tokio tasks.
///
/// Returns immediately; jobs run until the process exits.
pub fn start(state: AppState) {
    let cfg = EconomindConfig::load().unwrap_or_default();

    let bars_time = parse_hh_mm("ECONOMIND_SCHED_BARS_HH_MM", &cfg.schedule.bars_utc);
    let macro_time = parse_hh_mm("ECONOMIND_SCHED_MACRO_HH_MM", &cfg.schedule.macro_utc);
    let fund_time = parse_hh_mm("ECONOMIND_SCHED_FUND_HH_MM", &cfg.schedule.fundamentals_utc);
    let strategy_time = parse_hh_mm("ECONOMIND_SCHED_STRATEGY_HH_MM", &cfg.schedule.strategy_utc);
    let bars_lookback: i64 = std::env::var("ECONOMIND_SCHED_BARS_LOOKBACK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(cfg.schedule.bars_lookback_days as i64);

    if !cfg.schedule.enabled {
        info!("Scheduler disabled via config — no background jobs started");
        return;
    }

    // Build broker connector (optional — only needed when auto_execute configs exist).
    let broker: Option<Arc<dyn BrokerConnector>> = AlpacaConnector::from_env()
        .map(|c| Arc::new(c) as Arc<dyn BrokerConnector>)
        .map_err(|e| {
            info!("Alpaca connector unavailable ({e}) — order execution disabled");
        })
        .ok();

    let notifications = Arc::new(cfg.notifications.clone());
    let risk = cfg.risk.clone();

    tokio::spawn(job_bars(state.store().clone(), bars_time, bars_lookback));
    tokio::spawn(job_macro(state.store().clone(), macro_time));
    tokio::spawn(job_fundamentals(state.store().clone(), fund_time));
    tokio::spawn(job_strategy(
        state.clone(),
        strategy_time,
        broker,
        notifications,
        risk.max_drawdown_pct,
        risk.max_position_pct,
        risk.max_open_positions,
    ));

    info!(
        bars=%fmt_time(bars_time),
        macro_=%fmt_time(macro_time),
        fundamentals=%fmt_time(fund_time),
        strategy=%fmt_time(strategy_time),
        "Scheduler started"
    );
}

// ── Job: daily bar ingestion ──────────────────────────────────────────────────

async fn job_bars(store: DataStore, at: NaiveTime, lookback_days: i64) {
    loop {
        sleep_until(next_daily(at)).await;
        info!("Scheduler: starting bar ingestion");

        let since = Some((Utc::now() - chrono::Duration::days(lookback_days)).date_naive());
        let yahoo = YahooFinanceConnector::new().with_concurrency(4);
        let manager = DataFeedManager::new(DataFeedManagerConfig::default()).with_yahoo(yahoo);
        let result = manager.run_bars(&store, since).await;

        if result.symbols_err > 0 {
            warn!(
                ok = result.symbols_ok,
                err = result.symbols_err,
                "Scheduler: bar ingestion completed with errors"
            );
        } else {
            info!(ok = result.symbols_ok, "Scheduler: bar ingestion complete");
        }
    }
}

// ── Job: daily macro refresh ──────────────────────────────────────────────────

async fn job_macro(store: DataStore, at: NaiveTime) {
    loop {
        sleep_until(next_daily(at)).await;
        info!("Scheduler: starting macro ingestion");

        let fred = match FredConnector::from_env() {
            Ok(f) => f,
            Err(e) => {
                warn!("Scheduler: FRED connector unavailable ({e}), skipping macro ingestion");
                continue;
            }
        };
        let manager = DataFeedManager::new(DataFeedManagerConfig::default()).with_fred(fred);
        let result = manager.run_macro(&store, None).await;

        if result.symbols_err > 0 {
            warn!(
                ok = result.symbols_ok,
                err = result.symbols_err,
                "Scheduler: macro ingestion completed with errors"
            );
        } else {
            info!(
                ok = result.symbols_ok,
                "Scheduler: macro ingestion complete"
            );
        }
    }
}

// ── Job: weekly fundamentals refresh ─────────────────────────────────────────

async fn job_fundamentals(store: DataStore, at: NaiveTime) {
    loop {
        sleep_until(next_weekly(Weekday::Sun, at)).await;
        info!("Scheduler: starting fundamentals ingestion");

        let mut manager = DataFeedManager::new(DataFeedManagerConfig::default())
            .with_edgar(EdgarConnector::new());

        match SimFinConnector::from_env() {
            Ok(sf) => {
                manager = manager.with_simfin(sf);
            }
            Err(_) => {
                warn!("Scheduler: SIMFIN_API_KEY not set — running EDGAR only");
            }
        }

        let result = manager.run_fundamentals(&store).await;

        if result.symbols_err > 0 {
            warn!(
                ok = result.symbols_ok,
                err = result.symbols_err,
                "Scheduler: fundamentals ingestion completed with errors"
            );
        } else {
            info!(
                ok = result.symbols_ok,
                "Scheduler: fundamentals ingestion complete"
            );
        }
    }
}

// ── Job: daily strategy run + execution bridge ────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn job_strategy(
    state: AppState,
    at: NaiveTime,
    broker: Option<Arc<dyn BrokerConnector>>,
    notifications: Arc<NotificationsConfig>,
    max_drawdown_pct: f64,
    max_position_pct: f64,
    max_open_positions: usize,
) {
    loop {
        sleep_until(next_daily(at)).await;
        info!("Scheduler: starting strategy run");

        // Preload hot tables into memory before running strategies.
        if let Err(e) = state.store().preload(365).await {
            error!("Scheduler: preload failed: {e}");
            send_error_notification(&notifications, &format!("Preload failed: {e}")).await;
            continue;
        }

        // Load all enabled strategy configs.
        let configs = match state.store().list_strategy_configs().await {
            Ok(rows) => rows.into_iter().filter(|r| r.enabled).collect::<Vec<_>>(),
            Err(e) => {
                error!("Scheduler: failed to load strategy configs: {e}");
                continue;
            }
        };

        if configs.is_empty() {
            info!("Scheduler: no enabled strategy configs — nothing to run");
            continue;
        }

        for config_row in configs {
            let plugins: Vec<PluginSpec> = match serde_json::from_str(&config_row.plugins_json) {
                Ok(p) => p,
                Err(e) => {
                    error!(config=%config_row.id, "Scheduler: invalid plugins JSON: {e}");
                    continue;
                }
            };
            let parameters: HashMap<String, String> =
                match serde_json::from_str(&config_row.parameters_json) {
                    Ok(p) => p,
                    Err(e) => {
                        error!(config=%config_row.id, "Scheduler: invalid parameters JSON: {e}");
                        continue;
                    }
                };
            let composition = match config_row.composition.as_str() {
                "pipeline" => CompositionMode::Pipeline,
                "voting" => CompositionMode::Voting,
                "ensemble" => CompositionMode::Ensemble,
                other => {
                    error!(config=%config_row.id, "Scheduler: unknown composition '{other}'");
                    continue;
                }
            };
            let execution_mode = ExecutionMode::parse_lossy(&config_row.execution_mode);

            let strategy_config = StrategyConfig {
                id: config_row.id,
                name: config_row.name.clone(),
                description: config_row.description.clone(),
                composition,
                plugins,
                parameters,
                enabled: config_row.enabled,
                auto_execute: config_row.auto_execute,
                execution_mode,
                version: config_row.version,
                created_at: config_row.created_at,
                updated_at: config_row.updated_at,
            };

            let pipeline = match pipeline_factory::build_pipeline(&strategy_config) {
                Ok(p) => p,
                Err(e) => {
                    error!(config=%config_row.id, "Scheduler: pipeline build failed: {e}");
                    continue;
                }
            };

            let placeholder_run_id = Uuid::new_v4();
            state.event_bus().emit(ServerEvent::StrategyRunStarted {
                run_id: placeholder_run_id,
                config_id: strategy_config.id,
                started_at: Utc::now(),
            });

            let result = economind_strategy::orchestrator::run_strategy(
                &strategy_config,
                &pipeline,
                state.store(),
            )
            .await;

            let signal_count = result.signals.len();

            if matches!(result.status, economind_strategy::run::RunStatus::Failed) {
                error!(
                    config = %config_row.id,
                    error = ?result.error_message,
                    "Scheduler: strategy run failed"
                );
                let msg = result
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "unknown error".to_string());
                state.event_bus().emit(ServerEvent::SystemError {
                    message: msg.clone(),
                    occurred_at: Utc::now(),
                });
                send_error_notification(&notifications, &msg).await;
            } else {
                info!(
                    config = %config_row.id,
                    signals = signal_count,
                    "Scheduler: strategy run complete"
                );
                state.event_bus().emit(ServerEvent::StrategyRunCompleted {
                    run_id: result.run_id,
                    config_id: strategy_config.id,
                    signal_count,
                    completed_at: Utc::now(),
                });

                for sig in &result.signals {
                    state.event_bus().emit(ServerEvent::SignalEmitted {
                        signal_id: sig.id,
                        run_id: result.run_id,
                        symbol: sig.symbol.clone(),
                        direction: format!("{:?}", sig.direction),
                        timing_score: sig.timing_score,
                        emitted_at: sig.emitted_at,
                    });
                    if notifications.on_signal {
                        send_signal_notification(&notifications, sig, &strategy_config.name).await;
                    }
                }

                if notifications.on_run_complete {
                    send_run_complete_notification(
                        &notifications,
                        &strategy_config.name,
                        signal_count,
                    )
                    .await;
                }

                // ── Signal-to-order bridge ─────────────────────────────────────
                if strategy_config.auto_execute && execution_mode.executes_orders() {
                    if let Some(ref broker) = broker {
                        execute_signals(
                            broker.as_ref(),
                            state.store(),
                            &result.signals,
                            &strategy_config.name,
                            execution_mode,
                            &notifications,
                            max_drawdown_pct,
                            max_position_pct,
                            max_open_positions,
                        )
                        .await;
                    } else {
                        warn!(
                            config = %config_row.id,
                            "auto_execute=true but no broker connector available — skipping execution"
                        );
                    }
                }
            }
        }
    }
}

// ── Signal-to-order bridge ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn execute_signals(
    broker: &dyn BrokerConnector,
    store: &DataStore,
    signals: &[PersistedSignal],
    strategy_name: &str,
    execution_mode: ExecutionMode,
    notifications: &NotificationsConfig,
    max_drawdown_pct: f64,
    max_position_pct: f64,
    max_open_positions: usize,
) {
    // ── Risk pre-checks ───────────────────────────────────────────────────────

    // Load portfolio state from broker for authoritative values.
    let account = match broker.get_account().await {
        Ok(a) => a,
        Err(e) => {
            warn!("execute_signals: broker get_account failed ({e}) — skipping execution");
            return;
        }
    };

    let portfolio_value = account.portfolio_value;

    // Load DB portfolio state for drawdown (broker doesn't report it directly).
    let portfolio_state =
        store
            .load_portfolio_state()
            .await
            .unwrap_or_else(|_| economind_db::PortfolioState {
                open_positions: vec![],
                portfolio_value: Decimal::ZERO,
                available_cash: Decimal::ZERO,
                current_drawdown: Decimal::ZERO,
            });

    let drawdown_f64: f64 = portfolio_state.current_drawdown.try_into().unwrap_or(0.0);
    if drawdown_f64 >= max_drawdown_pct {
        warn!(
            drawdown=%portfolio_state.current_drawdown,
            limit=%max_drawdown_pct,
            "execute_signals: max drawdown breached — halting all order execution"
        );
        return;
    }

    let open_count = portfolio_state.open_positions.len();
    if open_count >= max_open_positions {
        warn!(
            open=%open_count,
            limit=%max_open_positions,
            "execute_signals: max_open_positions reached — skipping new orders"
        );
        return;
    }

    // ── Per-signal execution ──────────────────────────────────────────────────

    for sig in signals {
        // Position size check: skip if no shares computed.
        let shares = match sig.shares {
            Some(s) if s > Decimal::ZERO => s,
            _ => {
                info!(symbol=%sig.symbol.as_str(), "execute_signals: signal has no shares — skipping");
                continue;
            }
        };

        // Max position pct check.
        if portfolio_value > Decimal::ZERO {
            let notional = sig.notional.unwrap_or(shares * Decimal::from(100)); // rough fallback
            let fraction: f64 = (notional / portfolio_value).try_into().unwrap_or(1.0);
            if fraction > max_position_pct {
                warn!(
                    symbol=%sig.symbol.as_str(),
                    fraction=%fraction,
                    limit=%max_position_pct,
                    "execute_signals: position would exceed max_position_pct — skipping"
                );
                continue;
            }
        }

        let side = match sig.direction {
            economind_strategy::traits::TradeDirection::Long => OrderSide::Buy,
            economind_strategy::traits::TradeDirection::Short => OrderSide::Sell,
        };

        let sym_str = sig.symbol.as_str().to_string();

        let req = OrderRequest {
            signal_id: sig.id,
            symbol: sym_str.clone(),
            side,
            shares,
            note: Some(format!("strategy={strategy_name} mode={execution_mode}")),
        };

        info!(
            symbol=%sym_str,
            side=%side,
            shares=%shares,
            mode=%execution_mode,
            "execute_signals: submitting order"
        );

        match broker.submit_order(req).await {
            Ok(result) => {
                info!(
                    symbol=%result.symbol,
                    order_id=%result.broker_order_id,
                    status=?result.status,
                    "execute_signals: order submitted"
                );
                if notifications.on_order {
                    send_order_notification(
                        notifications,
                        &result.symbol,
                        side,
                        shares,
                        &result.broker_order_id,
                    )
                    .await;
                }
            }
            Err(e) => {
                error!(symbol=%sym_str, "execute_signals: order failed: {e}");
                send_error_notification(notifications, &format!("Order for {sym_str} failed: {e}"))
                    .await;
            }
        }
    }
}

// ── Notification helpers ──────────────────────────────────────────────────────

async fn send_webhook(url: &str, payload: serde_json::Value) {
    let client = reqwest::Client::new();
    if let Err(e) = client.post(url).json(&payload).send().await {
        warn!("Notification webhook failed: {e}");
    }
}

async fn send_signal_notification(
    cfg: &NotificationsConfig,
    sig: &PersistedSignal,
    strategy_name: &str,
) {
    let Some(url) = cfg.webhook_url.as_deref() else {
        return;
    };
    if url.is_empty() {
        return;
    }

    let payload = serde_json::json!({
        "event": "signal_emitted",
        "strategy": strategy_name,
        "signal_id": sig.id,
        "symbol": sig.symbol.as_str().to_string(),
        "direction": format!("{:?}", sig.direction),
        "timing_score": sig.timing_score,
        "emitted_at": sig.emitted_at.to_rfc3339(),
    });
    send_webhook(url, payload).await;
}

async fn send_run_complete_notification(
    cfg: &NotificationsConfig,
    strategy_name: &str,
    signal_count: usize,
) {
    let Some(url) = cfg.webhook_url.as_deref() else {
        return;
    };
    if url.is_empty() {
        return;
    }

    let payload = serde_json::json!({
        "event": "run_complete",
        "strategy": strategy_name,
        "signal_count": signal_count,
        "completed_at": Utc::now().to_rfc3339(),
    });
    send_webhook(url, payload).await;
}

async fn send_order_notification(
    cfg: &NotificationsConfig,
    symbol: &str,
    side: OrderSide,
    shares: Decimal,
    order_id: &str,
) {
    let Some(url) = cfg.webhook_url.as_deref() else {
        return;
    };
    if url.is_empty() {
        return;
    }

    let payload = serde_json::json!({
        "event": "order_submitted",
        "symbol": symbol,
        "side": side.to_string(),
        "shares": shares.to_string(),
        "broker_order_id": order_id,
        "submitted_at": Utc::now().to_rfc3339(),
    });
    send_webhook(url, payload).await;
}

async fn send_error_notification(cfg: &NotificationsConfig, message: &str) {
    if !cfg.on_error {
        return;
    }
    let Some(url) = cfg.webhook_url.as_deref() else {
        return;
    };
    if url.is_empty() {
        return;
    }

    let payload = serde_json::json!({
        "event": "error",
        "message": message,
        "occurred_at": Utc::now().to_rfc3339(),
    });
    send_webhook(url, payload).await;
}

// ── Time helpers ──────────────────────────────────────────────────────────────

fn next_daily(time: NaiveTime) -> Instant {
    let now_utc = Utc::now();
    let today = now_utc.date_naive();
    let candidate = today.and_time(time);
    let candidate_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(candidate, Utc);
    let target = if candidate_utc > now_utc {
        candidate_utc
    } else {
        let tomorrow = today + chrono::Duration::days(1);
        chrono::DateTime::<Utc>::from_naive_utc_and_offset(tomorrow.and_time(time), Utc)
    };
    let duration = (target - now_utc)
        .to_std()
        .unwrap_or(std::time::Duration::ZERO);
    Instant::now() + duration
}

fn next_weekly(weekday: Weekday, time: NaiveTime) -> Instant {
    let now_utc = Utc::now();
    let today = now_utc.date_naive();
    let today_weekday = today.weekday();
    let days_ahead = {
        let tw = weekday.num_days_from_monday();
        let cw = today_weekday.num_days_from_monday();
        let d = (tw + 7 - cw) % 7;
        if d == 0 {
            let candidate =
                chrono::DateTime::<Utc>::from_naive_utc_and_offset(today.and_time(time), Utc);
            if candidate <= now_utc {
                7
            } else {
                0
            }
        } else {
            d
        }
    };
    let target_date = today + chrono::Duration::days(days_ahead as i64);
    let target =
        chrono::DateTime::<Utc>::from_naive_utc_and_offset(target_date.and_time(time), Utc);
    let duration = (target - now_utc)
        .to_std()
        .unwrap_or(std::time::Duration::ZERO);
    Instant::now() + duration
}

fn parse_hh_mm(var: &str, default: &str) -> NaiveTime {
    let raw = std::env::var(var).unwrap_or_else(|_| default.to_string());
    NaiveTime::parse_from_str(&raw, "%H:%M").unwrap_or_else(|_| {
        warn!("{var}={raw:?} is not valid HH:MM — using default {default}");
        NaiveTime::parse_from_str(default, "%H:%M").unwrap()
    })
}

fn fmt_time(t: NaiveTime) -> String {
    format!("{:02}:{:02} UTC", t.hour(), t.minute())
}
