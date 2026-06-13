//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Server-side event bus — typed broadcast channel for WebSocket streaming (§5.D.2).
//!
//! The `EventBus` wraps a `tokio::sync::broadcast` channel.  Producers call
//! `emit()` to publish events; WebSocket handlers call `subscribe()` to receive
//! a `Receiver` they can poll in a loop.

use chrono::{DateTime, Utc};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

// ── Event types ───────────────────────────────────────────────────────────────

/// All events that can be streamed over the WebSocket signal channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// A strategy run has started.
    StrategyRunStarted {
        run_id: Uuid,
        config_id: Uuid,
        started_at: DateTime<Utc>,
    },
    /// A strategy run has completed.
    StrategyRunCompleted {
        run_id: Uuid,
        config_id: Uuid,
        signal_count: usize,
        completed_at: DateTime<Utc>,
    },
    /// A single trade signal was emitted during a strategy run.
    SignalEmitted {
        signal_id: Uuid,
        run_id: Uuid,
        symbol: Symbol,
        direction: String,
        timing_score: f64,
        emitted_at: DateTime<Utc>,
    },
    /// An ingestion job has completed.
    IngestionJobCompleted {
        job_name: String,
        records_written: u64,
        completed_at: DateTime<Utc>,
    },
    /// A live position was opened (future broker integration).
    PositionOpened {
        position_id: Uuid,
        symbol: Symbol,
        shares: Decimal,
        entry_price: Decimal,
        opened_at: DateTime<Utc>,
    },
    /// A live position was closed.
    PositionClosed {
        position_id: Uuid,
        symbol: Symbol,
        realized_pnl: Decimal,
        closed_at: DateTime<Utc>,
    },
    /// A system-level error occurred.
    SystemError {
        message: String,
        occurred_at: DateTime<Utc>,
    },
}

// ── EventBus ──────────────────────────────────────────────────────────────────

/// Cloneable handle to the broadcast event bus.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<ServerEvent>,
}

impl EventBus {
    pub fn new(tx: broadcast::Sender<ServerEvent>) -> Self {
        Self { tx }
    }

    /// Publish an event to all current subscribers.  Silently drops the event
    /// if there are no active subscribers (broadcast semantics).
    pub fn emit(&self, event: ServerEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to the event stream.  Returns a `Receiver` that will receive
    /// all events published *after* this call.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }
}
