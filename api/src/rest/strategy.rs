//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Strategy endpoints (§5.B.4):
//!
//! - `GET  /api/v1/strategy/configs`          — list all strategy configs
//! - `GET  /api/v1/strategy/configs/:id`      — single config
//! - `PUT  /api/v1/strategy/configs/:id`      — update parameters
//! - `POST /api/v1/strategy/run`              — trigger on-demand strategy run

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use economind_db::{StrategyConfigRow, StrategyStorage};
use economind_strategy::config::{CompositionMode, PluginSpec, StrategyConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    events::ServerEvent,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/strategy/configs", get(list_configs))
        .route("/strategy/configs/:id", get(get_config).put(update_config))
        .route("/strategy/run", post(trigger_run))
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StrategyConfigResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub composition: String,
    pub plugins: serde_json::Value,
    pub parameters: serde_json::Value,
    pub enabled: bool,
    pub version: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub parameters: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct TriggerRunRequest {
    pub config_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct TriggerRunResponse {
    pub run_id: Uuid,
    pub config_id: Uuid,
    pub status: String,
    pub started_at: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn row_to_response(row: StrategyConfigRow) -> ApiResult<StrategyConfigResponse> {
    let plugins: serde_json::Value = serde_json::from_str(&row.plugins_json)
        .map_err(|e| ApiError::Internal(format!("plugins JSON malformed: {e}")))?;
    let parameters: serde_json::Value = serde_json::from_str(&row.parameters_json)
        .map_err(|e| ApiError::Internal(format!("parameters JSON malformed: {e}")))?;
    Ok(StrategyConfigResponse {
        id: row.id,
        name: row.name,
        description: row.description,
        composition: row.composition,
        plugins,
        parameters,
        enabled: row.enabled,
        version: row.version,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /api/v1/strategy/configs`
async fn list_configs(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<StrategyConfigResponse>>> {
    let rows = state
        .store()
        .list_strategy_configs()
        .await
        .map_err(ApiError::Storage)?;

    let configs = rows
        .into_iter()
        .map(row_to_response)
        .collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(configs))
}

/// `GET /api/v1/strategy/configs/:id`
async fn get_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StrategyConfigResponse>> {
    let row = state
        .store()
        .get_strategy_config(id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(row_to_response(row)?))
}

/// `PUT /api/v1/strategy/configs/:id`
async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> ApiResult<Json<StrategyConfigResponse>> {
    let mut row = state
        .store()
        .get_strategy_config(id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    if let Some(name) = req.name {
        row.name = name;
    }
    if let Some(desc) = req.description {
        row.description = Some(desc);
    }
    if let Some(enabled) = req.enabled {
        row.enabled = enabled;
    }
    if let Some(params) = req.parameters {
        row.parameters_json = serde_json::to_string(&params)
            .map_err(|e| ApiError::Internal(format!("failed to serialize parameters: {e}")))?;
        row.version += 1;
    }
    row.updated_at = Utc::now();

    state
        .store()
        .update_strategy_config(&row)
        .await
        .map_err(ApiError::Storage)?;

    Ok(Json(row_to_response(row)?))
}

/// `POST /api/v1/strategy/run`
///
/// Triggers an on-demand strategy run for the given config ID.
/// `run_strategy` (in `economind-strategy`) handles all persistence internally.
async fn trigger_run(
    State(state): State<AppState>,
    Json(req): Json<TriggerRunRequest>,
) -> ApiResult<Json<TriggerRunResponse>> {
    // Load config row.
    let config_row = state
        .store()
        .get_strategy_config(req.config_id)
        .await
        .map_err(ApiError::Storage)?
        .ok_or(ApiError::NotFound)?;

    // Reconstruct StrategyConfig.
    let plugins: Vec<PluginSpec> = serde_json::from_str(&config_row.plugins_json)
        .map_err(|e| ApiError::BadRequest(format!("invalid plugins JSON: {e}")))?;
    let parameters: HashMap<String, String> =
        serde_json::from_str(&config_row.parameters_json)
            .map_err(|e| ApiError::BadRequest(format!("invalid parameters JSON: {e}")))?;
    let composition = match config_row.composition.as_str() {
        "pipeline" => CompositionMode::Pipeline,
        "voting"   => CompositionMode::Voting,
        "ensemble" => CompositionMode::Ensemble,
        other => return Err(ApiError::BadRequest(format!("unknown composition: {other}"))),
    };
    let strategy_config = StrategyConfig {
        id: config_row.id,
        name: config_row.name,
        description: config_row.description,
        composition,
        plugins,
        parameters,
        enabled: config_row.enabled,
        version: config_row.version,
        created_at: config_row.created_at,
        updated_at: config_row.updated_at,
    };

    // Build pipeline from registered plugins.
    let pipeline = crate::pipeline_factory::build_pipeline(&strategy_config)
        .map_err(|e| ApiError::BadRequest(format!("pipeline build failed: {e}")))?;

    let started_at = Utc::now();

    state.event_bus().emit(ServerEvent::StrategyRunStarted {
        run_id: Uuid::nil(), // placeholder — real ID assigned inside run_strategy
        config_id: req.config_id,
        started_at,
    });

    // run_strategy handles run-row persistence internally.
    let result = economind_strategy::run_strategy(
        &strategy_config,
        &pipeline,
        state.store(),
    )
    .await;

    state.event_bus().emit(ServerEvent::StrategyRunCompleted {
        run_id: result.run_id,
        config_id: req.config_id,
        signal_count: result.signal_count(),
        completed_at: result.completed_at,
    });

    Ok(Json(TriggerRunResponse {
        run_id: result.run_id,
        config_id: req.config_id,
        status: format!("{:?}", result.status).to_lowercase(),
        started_at: result.started_at.to_rfc3339(),
    }))
}
