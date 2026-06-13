//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Shared application state injected into every handler via Axum extractors.

use crate::events::EventBus;
use economind_agentic::ChatService;
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
    /// Present when `ANTHROPIC_API_KEY` is set at startup.
    pub chat_service: RwLock<Option<ChatService>>,
}

impl AppState {
    pub async fn new(duckdb_path: &str, api_key: String) -> anyhow::Result<Self> {
        let store = DataStore::open(duckdb_path)
            .map_err(|e| anyhow::anyhow!("DataStore open failed: {e}"))?;

        let chat_service = build_chat_service(&store).await?;

        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let event_bus = EventBus::new(tx);

        Ok(Self {
            inner: Arc::new(Inner {
                store,
                api_key,
                event_bus,
                chat_service: RwLock::new(chat_service),
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

    /// Returns the chat agent, or `None` if `ANTHROPIC_API_KEY` was not set.
    pub async fn chat_service(&self) -> Option<ChatService> {
        self.inner.chat_service.read().await.clone()
    }

    /// Rebuild the chat agent from persisted LLM settings and environment secrets.
    pub async fn reload_chat_service(&self) -> anyhow::Result<()> {
        let chat_service = build_chat_service(&self.inner.store).await?;
        *self.inner.chat_service.write().await = chat_service;
        Ok(())
    }
}

async fn build_chat_service(store: &DataStore) -> anyhow::Result<Option<ChatService>> {
    let model = store
        .get_setting("llm.anthropic_model")
        .await
        .map_err(|e| anyhow::anyhow!("LLM settings load failed: {e}"))?
        .or_else(|| std::env::var("AGENT_MODEL").ok())
        .unwrap_or_else(|| "claude-sonnet-4-6".to_string());

    Ok(ChatService::from_env_with_model(store.clone(), model))
}
