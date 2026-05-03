//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Shared application state injected into every handler via Axum extractors.

use crate::events::EventBus;
use economind_db::DataStore;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Capacity of the event broadcast channel (number of buffered events).
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
}

impl AppState {
    pub async fn new(
        database_url: &str,
        duckdb_path: &str,
        api_key: String,
    ) -> anyhow::Result<Self> {
        let store = DataStore::connect(database_url, duckdb_path)
            .await
            .map_err(|e| anyhow::anyhow!("DataStore connect failed: {e}"))?;

        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let event_bus = EventBus::new(tx);

        Ok(Self {
            inner: Arc::new(Inner { store, api_key, event_bus }),
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
}
