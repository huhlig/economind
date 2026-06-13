//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Rig-based chat agent for Economind.
//!
//! Provides a [`ChatService`] that wraps a rig `Agent` with five platform tools:
//! `get_signals`, `get_instrument`, `get_portfolio`, `query_bars`, and
//! `get_macro_context`.  The Anthropic backend is used; model is read from
//! `AGENT_MODEL` (default: `claude-sonnet-4-6`).
//!
//! # Environment variables
//!
//! | Variable            | Effect                                             |
//! |---------------------|----------------------------------------------------|
//! | `ANTHROPIC_API_KEY` | **Required** — used by `ChatService::from_env`     |
//! | `AGENT_MODEL`       | Override model (default: `claude-sonnet-4-6`)      |

use std::sync::Arc;

use anyhow::Result;
use chrono::NaiveDate;
use economind_core::model::{DailyCandleEntry, Symbol};
use economind_db::{
    CandleStorage, DataStore, MacroStorage, MetadataStorage, PortfolioStorage, StrategyStorage,
};
use futures::StreamExt;
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::message::{AssistantContent, Message, Text, UserContent};
use rig::completion::{Chat, ToolDefinition};
use rig::one_or_many::OneOrMany;
use rig::providers::anthropic;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::persona::{DisclosureContext, PersonaAgent, PersonaRegistry};

// ── System prompt ─────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "\
You are an expert quantitative analyst assistant embedded in the Economind \
low-frequency trading platform (trade horizons: 1 day – 1 month).

You have direct access to live platform data via tools:
- get_signals        — recent trade signals from active strategies
- get_instrument     — ticker metadata and recent OHLCV bars
- get_portfolio      — current positions, cash, and drawdown
- query_bars         — historical daily OHLCV data for any symbol
- get_macro_context  — latest FRED macro indicators (yields, CPI, VIX, etc.)

Guidelines:
- Always use tools to ground answers in real data before responding.
- Be concise, data-driven, and actionable.
- When citing numbers, name the source (e.g. \"latest signal from run X\").
- Flag stale or missing data explicitly.";

// ── Shared tool error type ────────────────────────────────────────────────────

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ToolError(String);

impl ToolError {
    pub(crate) fn new(e: impl std::fmt::Display) -> Self {
        ToolError(e.to_string())
    }
}

// ── Public API types ──────────────────────────────────────────────────────────

/// A single turn in a chat conversation, shared with the REST layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// `"user"` or `"assistant"`.
    pub role: String,
    pub content: String,
}

// ── Helper: build a user Message ──────────────────────────────────────────────

fn user_msg(text: impl Into<String>) -> Message {
    Message::User {
        content: OneOrMany::one(UserContent::Text(Text::new(text))),
    }
}

// ── Tool: get_signals ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct GetSignalsArgs {
    symbol: Option<String>,
    limit: Option<u32>,
}

pub(crate) struct GetSignalsTool {
    pub(crate) store: DataStore,
}

impl Tool for GetSignalsTool {
    const NAME: &'static str = "get_signals";
    type Error = ToolError;
    type Args = GetSignalsArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Query recent trade signals emitted by active strategies. \
                Returns direction, scores, position sizing, and rationale."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Filter by ticker symbol, e.g. \"AAPL\" (optional)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max signals to return (default 20)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run(args).await.map_err(ToolError::new)
    }
}

impl GetSignalsTool {
    async fn run(&self, args: GetSignalsArgs) -> Result<serde_json::Value> {
        let symbol = args.symbol.as_deref().map(Symbol::new);
        let signals = self
            .store
            .query_strategy_signals(None, None, symbol.as_ref(), None, args.limit.or(Some(20)))
            .await?;

        let rows: Vec<serde_json::Value> = signals
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "symbol": s.symbol,
                    "direction": s.direction,
                    "identifier_score": s.identifier_score,
                    "timing_score": s.timing_score,
                    "position_shares": s.position_shares,
                    "position_notional": s.position_notional,
                    "portfolio_fraction": s.portfolio_fraction,
                    "rationale": s.rationale,
                    "emitted_at": s.emitted_at,
                })
            })
            .collect();

        Ok(serde_json::json!({ "signals": rows, "count": rows.len() }))
    }
}

// ── Tool: get_instrument ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct GetInstrumentArgs {
    symbol: String,
    bars: Option<usize>,
}

pub(crate) struct GetInstrumentTool {
    pub(crate) store: DataStore,
}

impl Tool for GetInstrumentTool {
    const NAME: &'static str = "get_instrument";
    type Error = ToolError;
    type Args = GetInstrumentArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch metadata and recent daily OHLCV bars for a ticker symbol."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["symbol"],
                "properties": {
                    "symbol": { "type": "string", "description": "Ticker symbol, e.g. \"AAPL\"" },
                    "bars": { "type": "integer", "description": "Recent daily bars to return (default 20, max 100)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run(args).await.map_err(ToolError::new)
    }
}

impl GetInstrumentTool {
    async fn run(&self, args: GetInstrumentArgs) -> Result<serde_json::Value> {
        let symbol = Symbol::new(&args.symbol);
        let limit = args.bars.unwrap_or(20).min(100);

        let ticker = self.store.get_ticker(&symbol).await?;

        let today = chrono::Local::now().date_naive();
        let from = today - chrono::Duration::days((limit as i64) * 2);
        let bars: Vec<DailyCandleEntry> = self
            .store
            .query_daily_candles(&symbol, from..today)
            .await?
            .take(limit)
            .collect()
            .await;

        let bar_values: Vec<serde_json::Value> = bars
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "date": c.date, "open": c.open, "high": c.high,
                    "low": c.low, "close": c.close, "volume": c.volume,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "symbol": args.symbol,
            "ticker": ticker,
            "recent_bars": bar_values,
        }))
    }
}

// ── Tool: get_portfolio ───────────────────────────────────────────────────────

pub(crate) struct GetPortfolioTool {
    pub(crate) store: DataStore,
}

impl Tool for GetPortfolioTool {
    const NAME: &'static str = "get_portfolio";
    type Error = ToolError;
    type Args = serde_json::Value;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Return the current portfolio: open positions, total value, \
                available cash, and current drawdown from peak."
                .to_string(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run().await.map_err(ToolError::new)
    }
}

impl GetPortfolioTool {
    async fn run(&self) -> Result<serde_json::Value> {
        let state = self.store.load_portfolio_state().await?;

        let positions: Vec<serde_json::Value> = state
            .open_positions
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "symbol": p.symbol.as_str(),
                    "shares": p.shares,
                    "entry_price": p.entry_price,
                    "entry_at": p.entry_at,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "open_positions": positions,
            "portfolio_value": state.portfolio_value,
            "available_cash": state.available_cash,
            "current_drawdown": state.current_drawdown,
        }))
    }
}

// ── Tool: query_bars ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct QueryBarsArgs {
    symbol: String,
    from: String,
    to: String,
}

pub(crate) struct QueryBarsTool {
    pub(crate) store: DataStore,
}

impl Tool for QueryBarsTool {
    const NAME: &'static str = "query_bars";
    type Error = ToolError;
    type Args = QueryBarsArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch daily OHLCV bars for a symbol over a specific date range."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["symbol", "from", "to"],
                "properties": {
                    "symbol": { "type": "string" },
                    "from": { "type": "string", "description": "Start date YYYY-MM-DD (inclusive)" },
                    "to":   { "type": "string", "description": "End date YYYY-MM-DD (exclusive)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run(args).await.map_err(ToolError::new)
    }
}

impl QueryBarsTool {
    async fn run(&self, args: QueryBarsArgs) -> Result<serde_json::Value> {
        let symbol = Symbol::new(&args.symbol);
        let from = NaiveDate::parse_from_str(&args.from, "%Y-%m-%d")?;
        let to = NaiveDate::parse_from_str(&args.to, "%Y-%m-%d")?;

        let bars: Vec<DailyCandleEntry> = self
            .store
            .query_daily_candles(&symbol, from..to)
            .await?
            .collect()
            .await;

        let values: Vec<serde_json::Value> = bars
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "date": c.date, "open": c.open, "high": c.high,
                    "low": c.low, "close": c.close, "volume": c.volume,
                })
            })
            .collect();

        Ok(serde_json::json!({ "symbol": args.symbol, "bars": values }))
    }
}

// ── Tool: get_macro_context ───────────────────────────────────────────────────

pub(crate) struct GetMacroContextTool {
    pub(crate) store: DataStore,
}

impl Tool for GetMacroContextTool {
    const NAME: &'static str = "get_macro_context";
    type Error = ToolError;
    type Args = serde_json::Value;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Return latest FRED macro indicators: 10Y Treasury yield, \
                yield-curve spread (10Y–2Y), CPI, unemployment rate, VIX, and M2 money supply."
                .to_string(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run().await.map_err(ToolError::new)
    }
}

impl GetMacroContextTool {
    async fn run(&self) -> Result<serde_json::Value> {
        const SERIES: &[&str] = &["DGS10", "T10Y2Y", "CPIAUCSL", "UNRATE", "VIXCLS", "M2SL"];
        let points = self.store.get_latest_macro_values(SERIES).await?;

        let values: Vec<serde_json::Value> = points
            .into_iter()
            .map(|p| serde_json::json!({ "series_id": p.series_id, "date": p.date, "value": p.value }))
            .collect();

        Ok(serde_json::json!({ "macro_series": values }))
    }
}

// ── ChatService ───────────────────────────────────────────────────────────────

/// Drives an Economind chat agent backed by rig + Anthropic.
///
/// Use [`ChatService::from_env`] to construct — returns `None` when
/// `ANTHROPIC_API_KEY` is not set.
#[derive(Clone)]
pub struct ChatService {
    store: DataStore,
    client: anthropic::Client,
    model: String,
    /// Registered personas; shared across clones.
    pub personas: PersonaRegistry,
}

impl ChatService {
    /// Construct from an already-initialised Anthropic client.
    pub fn new(store: DataStore, client: anthropic::Client) -> Self {
        let model =
            std::env::var("AGENT_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        Self::new_with_model(store, client, model)
    }

    /// Construct from an already-initialised Anthropic client and explicit model.
    pub fn new_with_model(
        store: DataStore,
        client: anthropic::Client,
        model: impl Into<String>,
    ) -> Self {
        let model = model.into();

        let personas = PersonaRegistry::with_builtins();

        // Load JSON personas from the `personas/` directory next to the
        // working directory, or from PERSONAS_DIR env var if set.
        let personas_dir = std::env::var("PERSONAS_DIR").unwrap_or_else(|_| "personas".to_string());
        let (loaded, errors) = personas.load_dir(&personas_dir);
        if !loaded.is_empty() {
            info!(
                "Loaded {} persona(s) from {personas_dir}: {}",
                loaded.len(),
                loaded.join(", ")
            );
        }
        for e in errors {
            warn!("Persona load error: {e}");
        }

        Self {
            store,
            client,
            model,
            personas,
        }
    }

    /// Construct from environment variables.  Returns `None` if
    /// `ANTHROPIC_API_KEY` is not set or is invalid.
    pub fn from_env(store: DataStore) -> Option<Self> {
        let model =
            std::env::var("AGENT_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        Self::from_env_with_model(store, model)
    }

    /// Construct from environment variables with an explicit model override.
    pub fn from_env_with_model(store: DataStore, model: impl Into<String>) -> Option<Self> {
        let client = anthropic::Client::from_env()
            .map_err(|e| debug!("Chat agent disabled: {e}"))
            .ok()?;
        Some(Self::new_with_model(store, client, model))
    }

    /// Send `message` with optional prior `history` using the default
    /// (all-tools) agent.
    ///
    /// Returns the assistant's reply.  Callers are responsible for maintaining
    /// their own history — append `ChatMessage { role: "user", content: message }`
    /// and `ChatMessage { role: "assistant", content: reply }` after a successful call.
    pub async fn chat(&self, message: &str, history: Vec<ChatMessage>) -> Result<String> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(SYSTEM_PROMPT)
            .tool(GetSignalsTool {
                store: self.store.clone(),
            })
            .tool(GetInstrumentTool {
                store: self.store.clone(),
            })
            .tool(GetPortfolioTool {
                store: self.store.clone(),
            })
            .tool(QueryBarsTool {
                store: self.store.clone(),
            })
            .tool(GetMacroContextTool {
                store: self.store.clone(),
            })
            .max_tokens(4096)
            .build();

        let mut rig_history: Vec<Message> = history
            .into_iter()
            .map(|m| {
                if m.role == "assistant" {
                    Message::Assistant {
                        id: None,
                        content: OneOrMany::one(AssistantContent::Text(Text::new(m.content))),
                    }
                } else {
                    user_msg(m.content)
                }
            })
            .collect();

        let reply = agent
            .chat(user_msg(message), &mut rig_history)
            .await
            .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?;

        Ok(reply)
    }

    /// Send `message` through a named persona.
    ///
    /// The `persona_id` must match a persona registered in `self.personas`.
    /// Returns `Err` if the persona is not found.
    pub async fn chat_as(
        &self,
        persona_id: &str,
        message: &str,
        history: Vec<ChatMessage>,
        ctx: DisclosureContext,
    ) -> Result<String> {
        let persona = self
            .personas
            .get(persona_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown persona: {persona_id}"))?;

        let agent = PersonaAgent::new(
            persona,
            self.store.clone(),
            self.client.clone(),
            &self.model,
        )
        .with_registry(self.personas.clone());
        agent.chat(message, history, &ctx).await
    }

    /// List user-facing personas as `(id, name, description)` triples.
    pub fn list_personas(&self) -> Vec<(String, String, String)> {
        self.personas.list_visible()
    }

    /// Register a custom persona at runtime.
    pub fn register_persona(&self, persona: Arc<dyn crate::persona::Persona>) {
        self.personas.register(persona);
    }
}
