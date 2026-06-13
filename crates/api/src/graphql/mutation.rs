//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! GraphQL Mutation root (§5.C.3).

use async_graphql::{Context, Object, Result, ID};
use chrono::Utc;
use economind_core::model::Symbol;
use economind_db::StrategyStorage;
use economind_strategy::config::{CompositionMode, ExecutionMode, PluginSpec, StrategyConfig};
use std::collections::HashMap;
use uuid::Uuid;

use super::types::*;
use crate::{events::ServerEvent, state::AppState};

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    // ── Instruments ───────────────────────────────────────────────────────────

    /// Add an instrument to the tracked universe.
    async fn add_instrument(
        &self,
        ctx: &Context<'_>,
        symbol: String,
    ) -> Result<AddInstrumentResult> {
        let state = ctx.data::<AppState>()?;
        let sym = Symbol::new(&symbol);
        use economind_db::MetadataStorage;
        state.store().upsert_ticker(&sym).await?;
        Ok(AddInstrumentResult {
            symbol,
            status: "added".into(),
        })
    }

    /// Remove an instrument from the tracked universe.
    async fn remove_instrument(
        &self,
        ctx: &Context<'_>,
        symbol: String,
    ) -> Result<AddInstrumentResult> {
        let state = ctx.data::<AppState>()?;
        let sym = Symbol::new(&symbol);
        use economind_db::MetadataStorage;
        // Verify it exists.
        state
            .store()
            .get_ticker(&sym)
            .await?
            .ok_or_else(|| async_graphql::Error::new("instrument not found"))?;
        Ok(AddInstrumentResult {
            symbol,
            status: "removed".into(),
        })
    }

    // ── Strategy config ───────────────────────────────────────────────────────

    /// Update strategy config parameters, description, or enabled flag.
    async fn update_strategy_config(
        &self,
        ctx: &Context<'_>,
        id: ID,
        name: Option<String>,
        description: Option<String>,
        enabled: Option<bool>,
        parameters_json: Option<String>,
    ) -> Result<GqlStrategyConfig> {
        let state = ctx.data::<AppState>()?;
        let uid = Uuid::parse_str(&id)
            .map_err(|e| async_graphql::Error::new(format!("invalid id: {e}")))?;

        let mut row = state
            .store()
            .get_strategy_config(uid)
            .await?
            .ok_or_else(|| async_graphql::Error::new("strategy config not found"))?;

        if let Some(n) = name {
            row.name = n;
        }
        if let Some(d) = description {
            row.description = Some(d);
        }
        if let Some(e) = enabled {
            row.enabled = e;
        }
        if let Some(p) = parameters_json {
            // Validate JSON before saving.
            serde_json::from_str::<serde_json::Value>(&p)
                .map_err(|e| async_graphql::Error::new(format!("invalid parameters JSON: {e}")))?;
            row.parameters_json = p;
            row.version += 1;
        }
        row.updated_at = Utc::now();

        state.store().update_strategy_config(&row).await?;

        Ok(GqlStrategyConfig {
            id: row.id.to_string().into(),
            name: row.name,
            description: row.description,
            composition: row.composition,
            plugins: row.plugins_json,
            parameters: row.parameters_json,
            enabled: row.enabled,
            version: row.version as i32,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        })
    }

    // ── Strategy run ──────────────────────────────────────────────────────────

    /// Trigger an on-demand strategy run for a config ID.
    async fn trigger_strategy_run(
        &self,
        ctx: &Context<'_>,
        config_id: ID,
    ) -> Result<TriggerRunResult> {
        let state = ctx.data::<AppState>()?;
        let cid = Uuid::parse_str(&config_id)
            .map_err(|e| async_graphql::Error::new(format!("invalid config_id: {e}")))?;

        let config_row = state
            .store()
            .get_strategy_config(cid)
            .await?
            .ok_or_else(|| async_graphql::Error::new("strategy config not found"))?;

        let plugins: Vec<PluginSpec> = serde_json::from_str(&config_row.plugins_json)
            .map_err(|e| async_graphql::Error::new(format!("invalid plugins JSON: {e}")))?;
        let parameters: HashMap<String, String> = serde_json::from_str(&config_row.parameters_json)
            .map_err(|e| async_graphql::Error::new(format!("invalid parameters JSON: {e}")))?;
        let composition = match config_row.composition.as_str() {
            "pipeline" => CompositionMode::Pipeline,
            "voting" => CompositionMode::Voting,
            "ensemble" => CompositionMode::Ensemble,
            other => {
                return Err(async_graphql::Error::new(format!(
                    "unknown composition: {other}"
                )))
            }
        };

        let strategy_config = StrategyConfig {
            id: config_row.id,
            name: config_row.name,
            description: config_row.description,
            composition,
            plugins,
            parameters,
            enabled: config_row.enabled,
            auto_execute: config_row.auto_execute,
            execution_mode: ExecutionMode::parse_lossy(&config_row.execution_mode),
            version: config_row.version,
            created_at: config_row.created_at,
            updated_at: config_row.updated_at,
        };

        let started_at = Utc::now();

        // Build the pipeline from compiled plugins.
        let pipeline = crate::pipeline_factory::build_pipeline(&strategy_config)
            .map_err(|e| async_graphql::Error::new(format!("pipeline build failed: {e}")))?;

        state.event_bus().emit(ServerEvent::StrategyRunStarted {
            run_id: Uuid::nil(), // real run_id assigned inside run_strategy
            config_id: cid,
            started_at,
        });

        // run_strategy handles all persistence internally.
        let result =
            economind_strategy::run_strategy(&strategy_config, &pipeline, state.store()).await;

        state.event_bus().emit(ServerEvent::StrategyRunCompleted {
            run_id: result.run_id,
            config_id: cid,
            signal_count: result.signal_count(),
            completed_at: result.completed_at,
        });

        Ok(TriggerRunResult {
            run_id: result.run_id.to_string().into(),
            config_id: cid.to_string().into(),
            status: format!("{:?}", result.status).to_lowercase(),
            started_at: result.started_at.to_rfc3339(),
        })
    }
}
