//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Shared application state injected into every handler via Axum extractors.

use crate::events::EventBus;
use economind_agentic::ChatService;
use economind_db::DataStore;
use std::sync::Arc;
use tokio::sync::broadcast;

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
    pub chat_service: Option<ChatService>,
}

impl AppState {
    pub async fn new(duckdb_path: &str, api_key: String) -> anyhow::Result<Self> {
        let store = DataStore::open(duckdb_path)
            .map_err(|e| anyhow::anyhow!("DataStore open failed: {e}"))?;

        let chat_service = ChatService::from_env(store.clone());

        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let event_bus = EventBus::new(tx);

        Ok(Self {
            inner: Arc::new(Inner {
                store,
                api_key,
                event_bus,
                chat_service,
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
    pub fn chat_service(&self) -> Option<&ChatService> {
        self.inner.chat_service.as_ref()
    }
}
