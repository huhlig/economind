//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Manual scheduler trigger endpoints.
//!
//! These run the same ingestion logic as the background scheduler on-demand.
//!
//! - `GET  /api/v1/scheduler/status`              — next-fire times and enabled state
//! - `POST /api/v1/scheduler/trigger/bars`        — run bar ingestion now
//! - `POST /api/v1/scheduler/trigger/macro`       — run macro ingestion now
//! - `POST /api/v1/scheduler/trigger/fundamentals`— run fundamentals ingestion now

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use economind_config::EconomindConfig;
use economind_ingest::{
    DataFeedManager, DataFeedManagerConfig, EdgarConnector, FredConnector, SimFinConnector,
    YahooFinanceConnector,
};
use serde::Serialize;
use tracing::info;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scheduler/status", get(scheduler_status))
        .route("/scheduler/trigger/{job}", post(trigger_job))
}

#[derive(Debug, Serialize)]
pub struct SchedulerStatusResponse {
    pub enabled: bool,
    pub bars_utc: String,
    pub macro_utc: String,
    pub fundamentals_utc: String,
    pub strategy_utc: String,
    pub bars_lookback_days: u32,
}

#[derive(Debug, Serialize)]
pub struct TriggerResponse {
    pub job: String,
    pub status: String,
    pub summary: String,
}

async fn scheduler_status(State(_state): State<AppState>) -> ApiResult<Json<SchedulerStatusResponse>> {
    let cfg = EconomindConfig::load().unwrap_or_default();
    Ok(Json(SchedulerStatusResponse {
        enabled: cfg.schedule.enabled,
        bars_utc: cfg.schedule.bars_utc.clone(),
        macro_utc: cfg.schedule.macro_utc.clone(),
        fundamentals_utc: cfg.schedule.fundamentals_utc.clone(),
        strategy_utc: cfg.schedule.strategy_utc.clone(),
        bars_lookback_days: cfg.schedule.bars_lookback_days,
    }))
}

async fn trigger_job(
    State(state): State<AppState>,
    Path(job): Path<String>,
) -> ApiResult<Json<TriggerResponse>> {
    match job.as_str() {
        "bars" => {
            info!("Manual trigger: bar ingestion");
            let cfg = EconomindConfig::load().unwrap_or_default();
            let lookback = cfg.schedule.bars_lookback_days;
            let since = Some(
                (chrono::Utc::now() - chrono::Duration::days(lookback as i64)).date_naive(),
            );
            let yahoo = YahooFinanceConnector::new().with_concurrency(4);
            let manager = DataFeedManager::new(DataFeedManagerConfig::default()).with_yahoo(yahoo);
            let result = manager.run_bars(state.store(), since).await;
            info!(ok = result.symbols_ok, err = result.symbols_err, "Manual trigger: bar ingestion complete");
            Ok(Json(TriggerResponse {
                job: "bars".into(),
                status: if result.symbols_err > 0 && result.symbols_ok == 0 { "error".into() } else { "completed".into() },
                summary: result.to_string(),
            }))
        }
        "macro" => {
            info!("Manual trigger: macro ingestion");
            let fred = FredConnector::from_env()
                .map_err(|e| ApiError::BadRequest(format!("FRED_API_KEY not set: {e}")))?;
            let manager = DataFeedManager::new(DataFeedManagerConfig::default()).with_fred(fred);
            let result = manager.run_macro(state.store(), None).await;
            info!(ok = result.symbols_ok, err = result.symbols_err, "Manual trigger: macro ingestion complete");
            Ok(Json(TriggerResponse {
                job: "macro".into(),
                status: if result.symbols_err > 0 && result.symbols_ok == 0 { "error".into() } else { "completed".into() },
                summary: result.to_string(),
            }))
        }
        "fundamentals" => {
            info!("Manual trigger: fundamentals ingestion");
            let mut manager = DataFeedManager::new(DataFeedManagerConfig::default())
                .with_edgar(EdgarConnector::new());
            match SimFinConnector::from_env() {
                Ok(sf) => { manager = manager.with_simfin(sf); }
                Err(_) => { info!("SIMFIN_API_KEY not set — running EDGAR only"); }
            }
            let result = manager.run_fundamentals(state.store()).await;
            info!(ok = result.symbols_ok, err = result.symbols_err, "Manual trigger: fundamentals ingestion complete");
            Ok(Json(TriggerResponse {
                job: "fundamentals".into(),
                status: if result.symbols_err > 0 && result.symbols_ok == 0 { "error".into() } else { "completed".into() },
                summary: result.to_string(),
            }))
        }
        _ => Err(ApiError::NotFound),
    }
}
