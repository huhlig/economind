//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Runtime settings endpoints.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use economind_config::{IngestConfig, LlmConfig, NotificationsConfig, RiskConfig, ScheduleConfig};
use serde::{Deserialize, Serialize};

// ── LLM ───────────────────────────────────────────────────────────────────────

const KEY_PROVIDER: &str = "llm.provider";
const KEY_ANTHROPIC_MODEL: &str = "llm.anthropic_model";
const KEY_LOCAL_BASE_URL: &str = "llm.local_base_url";
const KEY_LOCAL_MODEL: &str = "llm.local_model";

#[derive(Debug, Serialize)]
pub struct LlmSettingsResponse {
    pub provider: String,
    pub anthropic_model: String,
    pub local_base_url: String,
    pub local_model: String,
    pub anthropic_api_key_configured: bool,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLlmSettingsRequest {
    pub provider: String,
    pub anthropic_model: String,
    pub local_base_url: String,
    pub local_model: String,
}

async fn get_llm_settings(State(state): State<AppState>) -> ApiResult<Json<LlmSettingsResponse>> {
    Ok(Json(load_llm_settings(&state).await?))
}

async fn update_llm_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateLlmSettingsRequest>,
) -> ApiResult<Json<LlmSettingsResponse>> {
    let provider = req.provider.trim().to_lowercase();
    if !matches!(provider.as_str(), "auto" | "anthropic" | "local") {
        return Err(ApiError::BadRequest(
            "provider must be auto, anthropic, or local".to_string(),
        ));
    }

    let anthropic_model = req.anthropic_model.trim();
    let local_base_url = req.local_base_url.trim();
    let local_model = req.local_model.trim();

    if anthropic_model.is_empty() {
        return Err(ApiError::BadRequest(
            "anthropic model cannot be empty".to_string(),
        ));
    }
    if local_base_url.is_empty() {
        return Err(ApiError::BadRequest(
            "local base URL cannot be empty".to_string(),
        ));
    }
    if local_model.is_empty() {
        return Err(ApiError::BadRequest("local model cannot be empty".to_string()));
    }

    state.store().set_setting(KEY_PROVIDER, &provider).await?;
    state.store().set_setting(KEY_ANTHROPIC_MODEL, anthropic_model).await?;
    state.store().set_setting(KEY_LOCAL_BASE_URL, local_base_url).await?;
    state.store().set_setting(KEY_LOCAL_MODEL, local_model).await?;
    state.reload_chat_service().await?;

    Ok(Json(load_llm_settings(&state).await?))
}

async fn load_llm_settings(state: &AppState) -> ApiResult<LlmSettingsResponse> {
    let defaults = LlmConfig::default();

    let provider = setting_or_env(state, KEY_PROVIDER, "LLM_PROVIDER", &defaults.provider).await?;
    let anthropic_model = setting_or_env(state, KEY_ANTHROPIC_MODEL, "AGENT_MODEL", &defaults.anthropic_model).await?;
    let local_base_url = setting_or_env(state, KEY_LOCAL_BASE_URL, "LOCAL_LLM_BASE_URL", &defaults.local_base_url).await?;
    let local_model = setting_or_env(state, KEY_LOCAL_MODEL, "LOCAL_LLM_MODEL", &defaults.local_model).await?;

    Ok(LlmSettingsResponse {
        provider,
        anthropic_model,
        local_base_url,
        local_model,
        anthropic_api_key_configured: std::env::var("ANTHROPIC_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        source: "duckdb".to_string(),
    })
}

// ── Datafeed / Ingest ─────────────────────────────────────────────────────────

const KEY_BAR_CONCURRENCY: &str = "ingest.bar_concurrency";
const KEY_BAR_BACKFILL_DAYS: &str = "ingest.bar_backfill_days";
const KEY_FRED_SERIES: &str = "ingest.fred_series";

#[derive(Debug, Serialize)]
pub struct DatafeedSettingsResponse {
    pub bar_concurrency: usize,
    pub bar_backfill_days: u32,
    pub fred_series: Vec<String>,
    pub alpaca_key_configured: bool,
    pub tiingo_key_configured: bool,
    pub simfin_key_configured: bool,
    pub fred_key_configured: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDatafeedSettingsRequest {
    pub bar_concurrency: usize,
    pub bar_backfill_days: u32,
    pub fred_series: Vec<String>,
}

async fn get_datafeed_settings(
    State(state): State<AppState>,
) -> ApiResult<Json<DatafeedSettingsResponse>> {
    Ok(Json(load_datafeed_settings(&state).await?))
}

async fn update_datafeed_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateDatafeedSettingsRequest>,
) -> ApiResult<Json<DatafeedSettingsResponse>> {
    if req.bar_concurrency == 0 {
        return Err(ApiError::BadRequest("bar_concurrency must be at least 1".to_string()));
    }
    if req.bar_backfill_days == 0 {
        return Err(ApiError::BadRequest("bar_backfill_days must be at least 1".to_string()));
    }

    state.store().set_setting(KEY_BAR_CONCURRENCY, &req.bar_concurrency.to_string()).await?;
    state.store().set_setting(KEY_BAR_BACKFILL_DAYS, &req.bar_backfill_days.to_string()).await?;
    state.store().set_setting(KEY_FRED_SERIES, &req.fred_series.join(",")).await?;

    Ok(Json(load_datafeed_settings(&state).await?))
}

async fn load_datafeed_settings(state: &AppState) -> ApiResult<DatafeedSettingsResponse> {
    let defaults = IngestConfig::default();

    let bar_concurrency = state.store().get_setting(KEY_BAR_CONCURRENCY).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(defaults.bar_concurrency);

    let bar_backfill_days = state.store().get_setting(KEY_BAR_BACKFILL_DAYS).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(defaults.bar_backfill_days);

    let fred_series = state.store().get_setting(KEY_FRED_SERIES).await?
        .map(|v| v.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect())
        .unwrap_or_default();

    Ok(DatafeedSettingsResponse {
        bar_concurrency,
        bar_backfill_days,
        fred_series,
        alpaca_key_configured: env_configured("APCA_API_KEY_ID"),
        tiingo_key_configured: env_configured("TIINGO_API_KEY"),
        simfin_key_configured: env_configured("SIMFIN_API_KEY"),
        fred_key_configured: env_configured("FRED_API_KEY"),
    })
}

fn env_configured(key: &str) -> bool {
    std::env::var(key).map(|v| !v.trim().is_empty()).unwrap_or(false)
}

// ── Schedule ──────────────────────────────────────────────────────────────────

const KEY_SCHED_ENABLED: &str = "schedule.enabled";
const KEY_SCHED_BARS_UTC: &str = "schedule.bars_utc";
const KEY_SCHED_MACRO_UTC: &str = "schedule.macro_utc";
const KEY_SCHED_FUNDAMENTALS_UTC: &str = "schedule.fundamentals_utc";
const KEY_SCHED_STRATEGY_UTC: &str = "schedule.strategy_utc";
const KEY_SCHED_BARS_LOOKBACK: &str = "schedule.bars_lookback_days";

#[derive(Debug, Serialize)]
pub struct ScheduleSettingsResponse {
    pub enabled: bool,
    pub bars_utc: String,
    pub macro_utc: String,
    pub fundamentals_utc: String,
    pub strategy_utc: String,
    pub bars_lookback_days: u32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScheduleSettingsRequest {
    pub enabled: bool,
    pub bars_utc: String,
    pub macro_utc: String,
    pub fundamentals_utc: String,
    pub strategy_utc: String,
    pub bars_lookback_days: u32,
}

async fn get_schedule_settings(
    State(state): State<AppState>,
) -> ApiResult<Json<ScheduleSettingsResponse>> {
    Ok(Json(load_schedule_settings(&state).await?))
}

async fn update_schedule_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateScheduleSettingsRequest>,
) -> ApiResult<Json<ScheduleSettingsResponse>> {
    state.store().set_setting(KEY_SCHED_ENABLED, if req.enabled { "true" } else { "false" }).await?;
    state.store().set_setting(KEY_SCHED_BARS_UTC, req.bars_utc.trim()).await?;
    state.store().set_setting(KEY_SCHED_MACRO_UTC, req.macro_utc.trim()).await?;
    state.store().set_setting(KEY_SCHED_FUNDAMENTALS_UTC, req.fundamentals_utc.trim()).await?;
    state.store().set_setting(KEY_SCHED_STRATEGY_UTC, req.strategy_utc.trim()).await?;
    state.store().set_setting(KEY_SCHED_BARS_LOOKBACK, &req.bars_lookback_days.to_string()).await?;
    Ok(Json(load_schedule_settings(&state).await?))
}

async fn load_schedule_settings(state: &AppState) -> ApiResult<ScheduleSettingsResponse> {
    let d = ScheduleConfig::default();

    let enabled = state.store().get_setting(KEY_SCHED_ENABLED).await?
        .map(|v| v != "false")
        .unwrap_or(d.enabled);

    let bars_utc = state.store().get_setting(KEY_SCHED_BARS_UTC).await?.unwrap_or(d.bars_utc);
    let macro_utc = state.store().get_setting(KEY_SCHED_MACRO_UTC).await?.unwrap_or(d.macro_utc);
    let fundamentals_utc = state.store().get_setting(KEY_SCHED_FUNDAMENTALS_UTC).await?.unwrap_or(d.fundamentals_utc);
    let strategy_utc = state.store().get_setting(KEY_SCHED_STRATEGY_UTC).await?.unwrap_or(d.strategy_utc);
    let bars_lookback_days = state.store().get_setting(KEY_SCHED_BARS_LOOKBACK).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(d.bars_lookback_days);

    Ok(ScheduleSettingsResponse { enabled, bars_utc, macro_utc, fundamentals_utc, strategy_utc, bars_lookback_days })
}

// ── Risk ──────────────────────────────────────────────────────────────────────

const KEY_RISK_MAX_DRAWDOWN: &str = "risk.max_drawdown_pct";
const KEY_RISK_MAX_POSITION: &str = "risk.max_position_pct";
const KEY_RISK_MAX_OPEN: &str = "risk.max_open_positions";

#[derive(Debug, Serialize)]
pub struct RiskSettingsResponse {
    pub max_drawdown_pct: f64,
    pub max_position_pct: f64,
    pub max_open_positions: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRiskSettingsRequest {
    pub max_drawdown_pct: f64,
    pub max_position_pct: f64,
    pub max_open_positions: usize,
}

async fn get_risk_settings(State(state): State<AppState>) -> ApiResult<Json<RiskSettingsResponse>> {
    Ok(Json(load_risk_settings(&state).await?))
}

async fn update_risk_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateRiskSettingsRequest>,
) -> ApiResult<Json<RiskSettingsResponse>> {
    if !(0.0..=1.0).contains(&req.max_drawdown_pct) {
        return Err(ApiError::BadRequest("max_drawdown_pct must be between 0 and 1".to_string()));
    }
    if !(0.0..=1.0).contains(&req.max_position_pct) {
        return Err(ApiError::BadRequest("max_position_pct must be between 0 and 1".to_string()));
    }
    if req.max_open_positions == 0 {
        return Err(ApiError::BadRequest("max_open_positions must be at least 1".to_string()));
    }

    state.store().set_setting(KEY_RISK_MAX_DRAWDOWN, &req.max_drawdown_pct.to_string()).await?;
    state.store().set_setting(KEY_RISK_MAX_POSITION, &req.max_position_pct.to_string()).await?;
    state.store().set_setting(KEY_RISK_MAX_OPEN, &req.max_open_positions.to_string()).await?;
    Ok(Json(load_risk_settings(&state).await?))
}

async fn load_risk_settings(state: &AppState) -> ApiResult<RiskSettingsResponse> {
    let d = RiskConfig::default();

    let max_drawdown_pct = state.store().get_setting(KEY_RISK_MAX_DRAWDOWN).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(d.max_drawdown_pct);

    let max_position_pct = state.store().get_setting(KEY_RISK_MAX_POSITION).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(d.max_position_pct);

    let max_open_positions = state.store().get_setting(KEY_RISK_MAX_OPEN).await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(d.max_open_positions);

    Ok(RiskSettingsResponse { max_drawdown_pct, max_position_pct, max_open_positions })
}

// ── Notifications ─────────────────────────────────────────────────────────────

const KEY_NOTIF_WEBHOOK: &str = "notifications.webhook_url";
const KEY_NOTIF_ON_SIGNAL: &str = "notifications.on_signal";
const KEY_NOTIF_ON_RUN: &str = "notifications.on_run_complete";
const KEY_NOTIF_ON_ORDER: &str = "notifications.on_order";
const KEY_NOTIF_ON_ERROR: &str = "notifications.on_error";

#[derive(Debug, Serialize)]
pub struct NotificationsSettingsResponse {
    pub webhook_url: Option<String>,
    pub on_signal: bool,
    pub on_run_complete: bool,
    pub on_order: bool,
    pub on_error: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotificationsSettingsRequest {
    pub webhook_url: Option<String>,
    pub on_signal: bool,
    pub on_run_complete: bool,
    pub on_order: bool,
    pub on_error: bool,
}

async fn get_notifications_settings(
    State(state): State<AppState>,
) -> ApiResult<Json<NotificationsSettingsResponse>> {
    Ok(Json(load_notifications_settings(&state).await?))
}

async fn update_notifications_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateNotificationsSettingsRequest>,
) -> ApiResult<Json<NotificationsSettingsResponse>> {
    let webhook = req.webhook_url.as_deref().unwrap_or("").trim().to_string();
    state.store().set_setting(KEY_NOTIF_WEBHOOK, &webhook).await?;
    state.store().set_setting(KEY_NOTIF_ON_SIGNAL, bool_str(req.on_signal)).await?;
    state.store().set_setting(KEY_NOTIF_ON_RUN, bool_str(req.on_run_complete)).await?;
    state.store().set_setting(KEY_NOTIF_ON_ORDER, bool_str(req.on_order)).await?;
    state.store().set_setting(KEY_NOTIF_ON_ERROR, bool_str(req.on_error)).await?;
    Ok(Json(load_notifications_settings(&state).await?))
}

async fn load_notifications_settings(state: &AppState) -> ApiResult<NotificationsSettingsResponse> {
    let d = NotificationsConfig::default();

    let webhook_url = state.store().get_setting(KEY_NOTIF_WEBHOOK).await?
        .map(|v| if v.is_empty() { None } else { Some(v) })
        .unwrap_or(d.webhook_url);

    let on_signal = state.store().get_setting(KEY_NOTIF_ON_SIGNAL).await?
        .map(|v| v == "true")
        .unwrap_or(d.on_signal);

    let on_run_complete = state.store().get_setting(KEY_NOTIF_ON_RUN).await?
        .map(|v| v == "true")
        .unwrap_or(d.on_run_complete);

    let on_order = state.store().get_setting(KEY_NOTIF_ON_ORDER).await?
        .map(|v| v == "true")
        .unwrap_or(d.on_order);

    let on_error = state.store().get_setting(KEY_NOTIF_ON_ERROR).await?
        .map(|v| v == "true")
        .unwrap_or(d.on_error);

    Ok(NotificationsSettingsResponse { webhook_url, on_signal, on_run_complete, on_order, on_error })
}

fn bool_str(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn setting_or_env(
    state: &AppState,
    setting_key: &str,
    env_key: &str,
    default: &str,
) -> ApiResult<String> {
    if let Some(value) = state.store().get_setting(setting_key).await? {
        return Ok(value);
    }
    Ok(std::env::var(env_key).unwrap_or_else(|_| default.to_string()))
}

// ── LLM Test ──────────────────────────────────────────────────────────────────

async fn test_llm(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    // Build a fresh agent from current DB settings so the test reflects what's
    // configured right now, not just what was saved at last restart.
    let agent = crate::state::build_chat_service(state.store())
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let Some(agent) = agent else {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "error": "No LLM backend configured — set provider and save, or check credentials"
        })));
    };

    match agent.chat("Reply with exactly: OK", vec![]).await {
        Ok(reply) => Ok(Json(serde_json::json!({
            "ok": true,
            "message": reply.chars().take(120).collect::<String>()
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "error": e.to_string()
        }))),
    }
}

// ── LLM Models ─────────────────────────────────────────────────────────────────

const ANTHROPIC_MODELS: &[&str] = &[
    "claude-haiku-4-5",
    "claude-sonnet-4-6",
    "claude-opus-4-8",
];

async fn list_llm_models(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let defaults = LlmConfig::default();
    let provider = setting_or_env(&state, KEY_PROVIDER, "LLM_PROVIDER", &defaults.provider).await?;
    let local_base_url = setting_or_env(&state, KEY_LOCAL_BASE_URL, "LOCAL_LLM_BASE_URL", &defaults.local_base_url).await?;

    match provider.as_str() {
        "anthropic" => Ok(Json(serde_json::json!({
            "provider": "anthropic",
            "models": ANTHROPIC_MODELS,
        }))),
        "local" => {
            let models = fetch_local_models(&local_base_url).await;
            Ok(Json(serde_json::json!({
                "provider": "local",
                "models": models,
            })))
        }
        _ => {
            // auto: return anthropic models (hardcoded) + local models if reachable
            let local_models = if !local_base_url.is_empty() {
                fetch_local_models(&local_base_url).await
            } else {
                vec![]
            };
            let anthropic_key_set = std::env::var("ANTHROPIC_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false);
            let models: Vec<serde_json::Value> = if anthropic_key_set {
                ANTHROPIC_MODELS.iter().map(|m| serde_json::json!({"id": m, "provider": "anthropic"})).collect()
            } else {
                local_models.iter().map(|m| serde_json::json!({"id": m, "provider": "local"})).collect()
            };
            Ok(Json(serde_json::json!({
                "provider": "auto",
                "models": models,
            })))
        }
    }
}

async fn fetch_local_models(base_url: &str) -> Vec<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    let base = base_url.trim_end_matches('/');

    // Try OpenAI-compatible /v1/models first
    if let Ok(resp) = client.get(format!("{base}/v1/models")).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(data) = json["data"].as_array() {
                let models: Vec<String> = data
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                    .collect();
                if !models.is_empty() {
                    return models;
                }
            }
        }
    }

    // Try Ollama /api/tags
    if let Ok(resp) = client.get(format!("{base}/api/tags")).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(models_arr) = json["models"].as_array() {
                let models: Vec<String> = models_arr
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                    .collect();
                if !models.is_empty() {
                    return models;
                }
            }
        }
    }

    vec![]
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings/llm", get(get_llm_settings).put(update_llm_settings))
        .route("/settings/llm/test", get(test_llm))
        .route("/settings/llm/models", get(list_llm_models))
        .route("/settings/datafeed", get(get_datafeed_settings).put(update_datafeed_settings))
        .route("/settings/schedule", get(get_schedule_settings).put(update_schedule_settings))
        .route("/settings/risk", get(get_risk_settings).put(update_risk_settings))
        .route("/settings/notifications", get(get_notifications_settings).put(update_notifications_settings))
}
