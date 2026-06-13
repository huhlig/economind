//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Shared application state injected into every handler via Axum extractors.

use crate::events::EventBus;
use economind_agentic::{ChatAgent, ChatService, LocalAgentChatService, PersonaRegistry};
use economind_config::LlmConfig;
use economind_db::DataStore;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

const EVENT_BUS_CAPACITY: usize = 256;

/// Cloneable handle to all shared server state.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    pub store: DataStore,
    pub api_key: String,
    pub event_bus: EventBus,
    /// Always populated — independent of LLM availability.
    pub personas: PersonaRegistry,
    /// Present when an LLM backend is configured.
    pub chat_agent: RwLock<Option<Arc<dyn ChatAgent>>>,
}

impl AppState {
    pub async fn new(duckdb_path: &str, api_key: String) -> anyhow::Result<Self> {
        let store = DataStore::open(duckdb_path)
            .map_err(|e| anyhow::anyhow!("DataStore open failed: {e}"))?;

        let chat_service = build_chat_service(&store).await?;
        let personas = build_persona_registry();

        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let event_bus = EventBus::new(tx);

        Ok(Self {
            inner: Arc::new(Inner {
                store,
                api_key,
                event_bus,
                personas,
                chat_agent: RwLock::new(chat_service),
            }),
        })
    }

    pub fn store(&self) -> &DataStore {
        &self.inner.store
    }

    pub fn api_key(&self) -> &str {
        &self.inner.api_key
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.inner.event_bus
    }

    /// Always-available persona registry (populated regardless of LLM config).
    pub fn personas(&self) -> &PersonaRegistry {
        &self.inner.personas
    }

    /// Returns the chat agent, or `None` if no LLM backend is configured.
    pub async fn chat_agent(&self) -> Option<Arc<dyn ChatAgent>> {
        self.inner.chat_agent.read().await.clone()
    }

    /// Rebuild the chat agent from persisted LLM settings and environment secrets.
    pub async fn reload_chat_service(&self) -> anyhow::Result<()> {
        let agent = build_chat_service(&self.inner.store).await?;
        *self.inner.chat_agent.write().await = agent;
        Ok(())
    }
}

fn build_persona_registry() -> PersonaRegistry {
    let registry = PersonaRegistry::with_builtins();
    let personas_dir =
        std::env::var("PERSONAS_DIR").unwrap_or_else(|_| "personas".to_string());
    let (loaded, errors) = registry.load_dir(&personas_dir);
    if !loaded.is_empty() {
        tracing::info!(
            "Loaded {} persona(s) from {personas_dir}: {}",
            loaded.len(),
            loaded.join(", ")
        );
    }
    for e in errors {
        tracing::warn!("Persona load error: {e}");
    }
    registry
}

pub async fn build_chat_service(
    store: &DataStore,
) -> anyhow::Result<Option<Arc<dyn ChatAgent>>> {
    let d = LlmConfig::default();

    let provider = store
        .get_setting("llm.provider")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("LLM_PROVIDER").ok())
        .unwrap_or(d.provider);

    let anthropic_model = store
        .get_setting("llm.anthropic_model")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("AGENT_MODEL").ok())
        .unwrap_or(d.anthropic_model);

    let local_base_url = store
        .get_setting("llm.local_base_url")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("LOCAL_LLM_BASE_URL").ok())
        .unwrap_or(d.local_base_url);

    let local_model = store
        .get_setting("llm.local_model")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("LOCAL_LLM_MODEL").ok())
        .unwrap_or(d.local_model);

    match provider.to_lowercase().as_str() {
        "local" => {
            if local_base_url.is_empty() {
                tracing::warn!("LLM provider=local but LOCAL_LLM_BASE_URL is not configured");
                return Ok(None);
            }
            tracing::info!("LLM backend: local at {local_base_url} model={local_model}");
            Ok(Some(Arc::new(LocalAgentChatService::new(store.clone(), local_base_url, local_model))))
        }
        "anthropic" => {
            match ChatService::from_env_with_model(store.clone(), anthropic_model) {
                Some(svc) => Ok(Some(Arc::new(svc))),
                None => {
                    tracing::warn!("LLM provider=anthropic but ANTHROPIC_API_KEY is not set");
                    Ok(None)
                }
            }
        }
        _ => {
            // auto: try Anthropic first, fall back to local
            if let Some(svc) = ChatService::from_env_with_model(store.clone(), anthropic_model) {
                tracing::info!("LLM backend: Anthropic (auto)");
                return Ok(Some(Arc::new(svc)));
            }
            if !local_base_url.is_empty() {
                tracing::info!("LLM backend: local (auto fallback) at {local_base_url}");
                return Ok(Some(Arc::new(LocalAgentChatService::new(store.clone(), local_base_url, local_model))));
            }
            tracing::debug!("No LLM backend configured — chat disabled");
            Ok(None)
        }
    }
}
