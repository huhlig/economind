//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Pluggable persona system with progressive skill disclosure.
//!
//! # Concepts
//!
//! - **[`Persona`]** — a trait describing an agent's identity, preamble, and which tools it
//!   exposes at each disclosure level.
//! - **[`DisclosureLevel`]** — an index (0 = minimal, increasing = more capable) computed
//!   from [`DisclosureContext`] (turn count, explicit depth requests, etc.).  Each persona
//!   decides its own thresholds.
//! - **[`PersonaRegistry`]** — a shareable map of `id → Arc<dyn Persona>` that can be
//!   extended at runtime by registering new persona implementations.
//! - **[`PersonaAgent`]** — combines a `Persona`, the platform `DataStore`, and an
//!   Anthropic client into a ready-to-run rig `Agent`.  Built fresh per chat turn so the
//!   tool set always reflects the current disclosure level.
//!
//! # Built-in personas
//!
//! | ID                  | Focus                                  |
//! |---------------------|----------------------------------------|
//! | `market_observer`   | Macro/market context, basic data       |
//! | `quant_analyst`     | Trade signals, strategy analysis       |
//! | `portfolio_manager` | Portfolio state, risk, sizing          |
//! | `data_explorer`     | Historical OHLCV deep dives            |
//!
//! # Example
//!
//! ```ignore
//! let registry = PersonaRegistry::default();          // all built-ins registered
//! registry.register(Arc::new(MyCustomPersona));       // add your own
//!
//! let persona = registry.get("quant_analyst").unwrap();
//! let agent   = PersonaAgent::new(persona, store.clone(), client.clone());
//! let reply   = agent.chat("What are the strongest signals?", history, ctx).await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use economind_db::DataStore;
use rig::client::CompletionClient;
use rig::completion::message::{AssistantContent, Message, Text, UserContent};
use rig::completion::Chat;
use rig::one_or_many::OneOrMany;
use rig::providers::anthropic;
use serde::{Deserialize, Serialize};

use rig::completion::ToolDefinition;
use rig::tool::Tool;

use crate::agent::{
    ChatMessage, GetInstrumentTool, GetMacroContextTool, GetPortfolioTool, GetSignalsTool,
    QueryBarsTool,
};

// ── Disclosure ────────────────────────────────────────────────────────────────

/// Snapshot of the current conversation used to compute disclosure level.
#[derive(Debug, Clone, Default)]
pub struct DisclosureContext {
    /// Number of completed turns in this conversation.
    pub turn_count: usize,
    /// Explicit depth the user requested ("basic" / "detailed" / "expert").
    pub requested_depth: Option<RequestedDepth>,
}

/// Caller-supplied depth hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestedDepth {
    Basic,
    Detailed,
    Expert,
}

/// Resolved disclosure index.  Higher = more tools / capabilities shown.
pub type DisclosureLevel = usize;

// ── Tool specification ────────────────────────────────────────────────────────

/// Which platform tools a persona can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSpec {
    GetSignals,
    GetInstrument,
    GetPortfolio,
    QueryBars,
    GetMacroContext,
}

/// A group of tools that unlock together at a disclosure threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLevel {
    /// First disclosure index at which these tools become available.
    pub min_level: DisclosureLevel,
    /// Human-readable label shown in docs / introspection.
    pub label: String,
    pub tools: Vec<ToolSpec>,
}

// ── Persona trait ─────────────────────────────────────────────────────────────

/// Trait every persona must implement.
///
/// Implementors define their identity, system prompt, and which tools to
/// expose at each disclosure level.  The runtime will call
/// [`Persona::resolve_level`] with the current [`DisclosureContext`] to
/// decide which levels to activate before building the agent.
pub trait Persona: Send + Sync + 'static {
    /// Stable, kebab-case identifier (e.g. `"quant_analyst"`).
    fn id(&self) -> &str;

    /// Short display name.
    fn name(&self) -> &str;

    /// One-sentence description used when presenting this persona to the user
    /// or when registering it as a sub-agent tool.
    fn description(&self) -> &str;

    /// System prompt injected as the agent's preamble.
    fn preamble(&self) -> String;

    /// All tool levels this persona can potentially expose, in ascending order
    /// of `min_level`.
    fn tool_levels(&self) -> Vec<ToolLevel>;

    /// IDs of other personas this persona is allowed to delegate work to.
    ///
    /// Returning a non-empty list causes a [`DelegateToPersona`] tool to be
    /// injected into this persona's agent, scoped to exactly these IDs.
    /// An empty list (the default) means no delegation is available.
    fn delegates(&self) -> Vec<String> { vec![] }

    /// Compute the active disclosure level from the current conversation
    /// context.  The default implementation uses turn count and depth hint.
    fn resolve_level(&self, ctx: &DisclosureContext) -> DisclosureLevel {
        match ctx.requested_depth {
            Some(RequestedDepth::Expert) => 2,
            Some(RequestedDepth::Detailed) => 1,
            Some(RequestedDepth::Basic) => 0,
            None => {
                if ctx.turn_count >= 8 {
                    2
                } else if ctx.turn_count >= 3 {
                    1
                } else {
                    0
                }
            }
        }
    }

    /// Return the active tool specs for a given context.
    fn active_tools(&self, ctx: &DisclosureContext) -> Vec<ToolSpec> {
        let level = self.resolve_level(ctx);
        self.tool_levels()
            .into_iter()
            .filter(|tl| tl.min_level <= level)
            .flat_map(|tl| tl.tools)
            .collect()
    }
}

// ── PersonaRegistry ───────────────────────────────────────────────────────────

/// Thread-safe registry of named personas.
///
/// Populated with all built-in personas on [`Default::default`]; additional
/// personas can be added with [`PersonaRegistry::register`].
#[derive(Clone, Default)]
pub struct PersonaRegistry {
    inner: Arc<std::sync::RwLock<HashMap<String, Arc<dyn Persona>>>>,
}

impl PersonaRegistry {
    /// Register a persona.  Overwrites any existing persona with the same ID.
    pub fn register(&self, persona: Arc<dyn Persona>) {
        self.inner
            .write()
            .expect("persona registry poisoned")
            .insert(persona.id().to_string(), persona);
    }

    /// Look up a persona by ID.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Persona>> {
        self.inner.read().expect("persona registry poisoned").get(id).cloned()
    }

    /// List all registered persona IDs and descriptions.
    pub fn list(&self) -> Vec<(String, String)> {
        self.inner
            .read()
            .expect("persona registry poisoned")
            .values()
            .map(|p| (p.id().to_string(), p.description().to_string()))
            .collect()
    }
}

// ── Default registry factory ──────────────────────────────────────────────────

impl PersonaRegistry {
    /// Build a registry pre-populated with all four built-in personas.
    pub fn with_builtins() -> Self {
        let r = Self::default();
        r.register(Arc::new(MarketObserverPersona));
        r.register(Arc::new(QuantAnalystPersona));
        r.register(Arc::new(PortfolioManagerPersona));
        r.register(Arc::new(DataExplorerPersona));
        r
    }
}

// ── PersonaAgent ─────────────────────────────────────────────────────────────

/// Combines a persona + DataStore + Anthropic client into a runnable agent.
pub struct PersonaAgent {
    persona: Arc<dyn Persona>,
    store: DataStore,
    client: anthropic::Client,
    model: String,
    registry: Option<PersonaRegistry>,
}

impl PersonaAgent {
    pub fn new(
        persona: Arc<dyn Persona>,
        store: DataStore,
        client: anthropic::Client,
        model: impl Into<String>,
    ) -> Self {
        Self {
            persona,
            store,
            client,
            model: model.into(),
            registry: None,
        }
    }

    /// Attach a registry so this persona can delegate to sub-agents.
    ///
    /// Required if the persona declares any `delegates()`.  Without a registry
    /// the delegate IDs are ignored at runtime.
    pub fn with_registry(mut self, registry: PersonaRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Run a single chat turn.
    ///
    /// The `ctx` controls which tool levels are unlocked for this turn.
    pub async fn chat(
        &self,
        message: &str,
        history: Vec<ChatMessage>,
        ctx: &DisclosureContext,
    ) -> Result<String> {
        let active = self.persona.active_tools(ctx);
        let preamble = self.persona.preamble();

        // Build a scoped DelegateToPersona tool if this persona declares delegates
        // and a registry is available.
        let delegate_tool = {
            let delegate_ids = self.persona.delegates();
            if !delegate_ids.is_empty() {
                if let Some(registry) = &self.registry {
                    // Build a sub-registry containing only the declared delegates.
                    let sub_registry = PersonaRegistry::default();
                    for id in &delegate_ids {
                        if let Some(p) = registry.get(id) {
                            sub_registry.register(p);
                        }
                    }
                    Some(DelegateToPersona::new(
                        sub_registry,
                        self.store.clone(),
                        self.client.clone(),
                        &self.model,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let reply = build_and_chat(
            &self.client,
            &self.model,
            &preamble,
            active,
            self.store.clone(),
            message,
            history,
            delegate_tool,
        )
        .await?;

        Ok(reply)
    }
}

// ── Agent construction helper ─────────────────────────────────────────────────

/// Build an agent from the active tool set and run one chat turn.
///
/// rig agents are generic over their tool set, so we cannot store a
/// pre-built agent polymorphically.  Instead we rebuild it per turn (cheap —
/// it's just struct construction) with exactly the tools that are active.
///
/// When `delegate` is `Some`, a [`DelegateToPersona`] tool is added to the
/// agent regardless of disclosure level, enabling sub-agent calls.
async fn build_and_chat(
    client: &anthropic::Client,
    model: &str,
    preamble: &str,
    active: Vec<ToolSpec>,
    store: DataStore,
    message: &str,
    history: Vec<ChatMessage>,
    delegate: Option<DelegateToPersona>,
) -> Result<String> {
    let level = if active.contains(&ToolSpec::GetPortfolio) && active.contains(&ToolSpec::GetSignals) {
        2 // full
    } else if active.contains(&ToolSpec::GetSignals) || active.contains(&ToolSpec::QueryBars) {
        1 // signals + market
    } else {
        0 // market only
    };

    let mut rig_history: Vec<Message> = history
        .into_iter()
        .map(|m| {
            if m.role == "assistant" {
                Message::Assistant {
                    id: None,
                    content: OneOrMany::one(AssistantContent::Text(Text::new(m.content))),
                }
            } else {
                Message::User {
                    content: OneOrMany::one(UserContent::Text(Text::new(m.content))),
                }
            }
        })
        .collect();

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text::new(message))),
    };

    // rig agents are monomorphic over their tool set, so we must branch on
    // both level and whether a delegate tool is present.
    let reply = match (level, delegate) {
        (0, None) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store })
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
        (0, Some(dt)) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store })
                .tool(dt)
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
        (1, None) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store: store.clone() })
                .tool(GetSignalsTool { store: store.clone() })
                .tool(QueryBarsTool { store })
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
        (1, Some(dt)) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store: store.clone() })
                .tool(GetSignalsTool { store: store.clone() })
                .tool(QueryBarsTool { store })
                .tool(dt)
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
        (_, None) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store: store.clone() })
                .tool(GetSignalsTool { store: store.clone() })
                .tool(QueryBarsTool { store: store.clone() })
                .tool(GetPortfolioTool { store })
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
        (_, Some(dt)) => {
            client
                .agent(model)
                .preamble(preamble)
                .tool(GetInstrumentTool { store: store.clone() })
                .tool(GetMacroContextTool { store: store.clone() })
                .tool(GetSignalsTool { store: store.clone() })
                .tool(QueryBarsTool { store: store.clone() })
                .tool(GetPortfolioTool { store })
                .tool(dt)
                .max_tokens(4096)
                .build()
                .chat(prompt, &mut rig_history)
                .await
                .map_err(|e| anyhow::anyhow!("Agent error: {e}"))?
        }
    };

    Ok(reply)
}

// ── Built-in personas ─────────────────────────────────────────────────────────

/// Watches market conditions and macro indicators.  Never touches signals or
/// portfolio state — safe to expose to less-experienced users.
pub struct MarketObserverPersona;

impl Persona for MarketObserverPersona {
    fn id(&self) -> &str { "market_observer" }
    fn name(&self) -> &str { "Market Observer" }
    fn description(&self) -> &str {
        "Monitors market conditions and macro indicators. \
         Focused on context, not trade decisions."
    }

    fn preamble(&self) -> String {
        "You are the Market Observer — an Economind analyst focused exclusively on \
         macro-economic context and market conditions. You have access to live \
         instrument data and FRED macro series.\n\n\
         - Summarise market environment clearly and concisely.\n\
         - Do NOT discuss individual trade signals or portfolio decisions.\n\
         - Cite specific data points (date, value, source series) in every answer.\n\
         - Flag if data appears stale (older than 5 business days)."
            .to_string()
    }

    fn tool_levels(&self) -> Vec<ToolLevel> {
        vec![
            ToolLevel {
                min_level: 0,
                label: "Market data".to_string(),
                tools: vec![ToolSpec::GetInstrument, ToolSpec::GetMacroContext],
            },
            ToolLevel {
                min_level: 1,
                label: "Historical bars".to_string(),
                tools: vec![ToolSpec::QueryBars],
            },
        ]
    }
}

/// Deep signal and strategy analysis.
pub struct QuantAnalystPersona;

impl Persona for QuantAnalystPersona {
    fn id(&self) -> &str { "quant_analyst" }
    fn name(&self) -> &str { "Quant Analyst" }
    fn description(&self) -> &str {
        "Analyses trade signals, strategies, and price behaviour. \
         Answers quantitative questions about the platform's signal output."
    }

    fn preamble(&self) -> String {
        "You are the Quant Analyst — an Economind specialist in trade signal \
         analysis, strategy behaviour, and price dynamics.\n\n\
         - Ground every insight in real signal data, price history, or macro context.\n\
         - Distinguish between identifier score (setup quality) and timing score \
           (entry timing).\n\
         - Quantify claims: \"signal X has score 0.82, above the 0.75 historical \
           median\" — not just \"strong signal\".\n\
         - Escalate to Portfolio Manager if the user asks about sizing or risk."
            .to_string()
    }

    fn tool_levels(&self) -> Vec<ToolLevel> {
        vec![
            ToolLevel {
                min_level: 0,
                label: "Signals and instruments".to_string(),
                tools: vec![ToolSpec::GetSignals, ToolSpec::GetInstrument],
            },
            ToolLevel {
                min_level: 1,
                label: "Historical analysis".to_string(),
                tools: vec![ToolSpec::QueryBars, ToolSpec::GetMacroContext],
            },
        ]
    }
}

/// Portfolio state, risk, and position sizing.
pub struct PortfolioManagerPersona;

impl Persona for PortfolioManagerPersona {
    fn id(&self) -> &str { "portfolio_manager" }
    fn name(&self) -> &str { "Portfolio Manager" }
    fn description(&self) -> &str {
        "Manages portfolio state, risk metrics, drawdown, and position sizing."
    }

    fn preamble(&self) -> String {
        "You are the Portfolio Manager — an Economind specialist in portfolio \
         state, risk, and position sizing.\n\n\
         - Always start with the current portfolio snapshot before discussing trades.\n\
         - Highlight drawdown and cash position first.\n\
         - Refer to signal scores when evaluating entry candidates.\n\
         - Do NOT recommend trade execution — only analyse and advise.\n\
         - Quantify risk in dollar terms AND as a portfolio fraction."
            .to_string()
    }

    fn tool_levels(&self) -> Vec<ToolLevel> {
        vec![
            ToolLevel {
                min_level: 0,
                label: "Portfolio state".to_string(),
                tools: vec![ToolSpec::GetPortfolio, ToolSpec::GetSignals],
            },
            ToolLevel {
                min_level: 1,
                label: "Market context".to_string(),
                tools: vec![ToolSpec::GetInstrument, ToolSpec::GetMacroContext],
            },
            ToolLevel {
                min_level: 2,
                label: "Historical deep dive".to_string(),
                tools: vec![ToolSpec::QueryBars],
            },
        ]
    }
}

/// Historical OHLCV and data quality analysis.
pub struct DataExplorerPersona;

impl Persona for DataExplorerPersona {
    fn id(&self) -> &str { "data_explorer" }
    fn name(&self) -> &str { "Data Explorer" }
    fn description(&self) -> &str {
        "Explores historical price data, data coverage, and time-series patterns."
    }

    fn preamble(&self) -> String {
        "You are the Data Explorer — an Economind specialist in historical \
         price data, time-series analysis, and data quality.\n\n\
         - Fetch bars before drawing any conclusions about price behaviour.\n\
         - Comment on data gaps, missing bars, or suspiciously uniform prices.\n\
         - Express patterns quantitatively: ranges, return distributions, \
           volatility estimates.\n\
         - Keep macro context available but secondary to the price data."
            .to_string()
    }

    fn tool_levels(&self) -> Vec<ToolLevel> {
        vec![
            ToolLevel {
                min_level: 0,
                label: "Bar data".to_string(),
                tools: vec![ToolSpec::QueryBars, ToolSpec::GetInstrument],
            },
            ToolLevel {
                min_level: 1,
                label: "Macro overlay".to_string(),
                tools: vec![ToolSpec::GetMacroContext],
            },
        ]
    }
}

// ── FilePersona — JSON-loadable persona ───────────────────────────────────────

/// Data-driven persona definition that can be deserialized from a JSON file.
///
/// # File format
///
/// ```json
/// {
///   "id": "sector_analyst",
///   "name": "Sector Analyst",
///   "description": "Focuses on sector rotation and relative strength.",
///   "preamble": "You are a sector analyst...",
///   "tool_levels": [
///     { "min_level": 0, "label": "Basic data",  "tools": ["get_instrument", "get_macro_context"] },
///     { "min_level": 1, "label": "Signals",      "tools": ["get_signals"] },
///     { "min_level": 2, "label": "History",      "tools": ["query_bars"] }
///   ],
///   "escalation": { "level_1_after_turns": 3, "level_2_after_turns": 8 }
/// }
/// ```
///
/// `escalation` is optional; omitting it uses the same defaults as the built-in personas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePersona {
    pub id: String,
    pub name: String,
    pub description: String,
    pub preamble: String,
    pub tool_levels: Vec<ToolLevel>,
    #[serde(default)]
    pub escalation: EscalationConfig,
    /// Persona IDs this persona may delegate to.  Omit or leave empty for no delegation.
    #[serde(default)]
    pub delegates: Vec<String>,
}

/// Thresholds for automatic disclosure-level escalation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationConfig {
    /// Number of completed turns before level 1 unlocks automatically.
    #[serde(default = "default_level1_turns")]
    pub level_1_after_turns: usize,
    /// Number of completed turns before level 2 unlocks automatically.
    #[serde(default = "default_level2_turns")]
    pub level_2_after_turns: usize,
}

fn default_level1_turns() -> usize { 3 }
fn default_level2_turns() -> usize { 8 }

impl Default for EscalationConfig {
    fn default() -> Self {
        Self {
            level_1_after_turns: default_level1_turns(),
            level_2_after_turns: default_level2_turns(),
        }
    }
}

impl Persona for FilePersona {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn preamble(&self) -> String { self.preamble.clone() }
    fn tool_levels(&self) -> Vec<ToolLevel> { self.tool_levels.clone() }
    fn delegates(&self) -> Vec<String> { self.delegates.clone() }

    fn resolve_level(&self, ctx: &DisclosureContext) -> DisclosureLevel {
        match ctx.requested_depth {
            Some(RequestedDepth::Expert) => 2,
            Some(RequestedDepth::Detailed) => 1,
            Some(RequestedDepth::Basic) => 0,
            None => {
                if ctx.turn_count >= self.escalation.level_2_after_turns {
                    2
                } else if ctx.turn_count >= self.escalation.level_1_after_turns {
                    1
                } else {
                    0
                }
            }
        }
    }
}

impl FilePersona {
    /// Parse a `FilePersona` from a JSON string.
    pub fn from_json_str(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Load a `FilePersona` from a `.json` file on disk.
    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read persona file {}: {e}", path.display()))?;
        serde_json::from_str(&src)
            .map_err(|e| anyhow::anyhow!("Invalid persona JSON in {}: {e}", path.display()))
    }
}

// ── Registry file loaders ─────────────────────────────────────────────────────

impl PersonaRegistry {
    /// Load a single persona from a JSON file and register it.
    ///
    /// Returns the persona ID on success.
    pub fn load_file(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<String> {
        let persona = FilePersona::load(path)?;
        let id = persona.id.clone();
        self.register(Arc::new(persona));
        Ok(id)
    }

    /// Scan a directory for `*.json` files, load each as a [`FilePersona`],
    /// and register them.  Built-ins are never overwritten — file-loaded
    /// personas with the same ID as a built-in will replace it.
    ///
    /// Returns `(loaded, errors)` — errors are non-fatal so the caller can
    /// log them without aborting the whole scan.
    pub fn load_dir(
        &self,
        dir: impl AsRef<std::path::Path>,
    ) -> (Vec<String>, Vec<anyhow::Error>) {
        let dir = dir.as_ref();
        let mut loaded = Vec::new();
        let mut errors = Vec::new();

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                errors.push(anyhow::anyhow!("Cannot read persona dir {}: {e}", dir.display()));
                return (loaded, errors);
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match self.load_file(&path) {
                Ok(id) => loaded.push(id),
                Err(e) => errors.push(e),
            }
        }

        (loaded, errors)
    }
}

// ── DelegateToPersona — subagent tool ─────────────────────────────────────────

/// A rig `Tool` that the orchestrator agent can call to delegate a question to
/// any registered persona.  This is what enables the "subagent" pattern: the
/// root agent decides *which* specialist to route to, passes the sub-question,
/// and returns the specialist's answer as part of its own reply.
///
/// To add this to an orchestrator agent:
/// ```ignore
/// client.agent(model)
///     .preamble(ORCHESTRATOR_PREAMBLE)
///     .tool(DelegateToPersona::new(registry, store, client, model))
///     .build()
/// ```
pub struct DelegateToPersona {
    registry: PersonaRegistry,
    store: DataStore,
    client: anthropic::Client,
    model: String,
}

impl DelegateToPersona {
    pub fn new(
        registry: PersonaRegistry,
        store: DataStore,
        client: anthropic::Client,
        model: impl Into<String>,
    ) -> Self {
        Self { registry, store, client, model: model.into() }
    }
}

#[derive(Deserialize)]
pub struct DelegateArgs {
    /// Persona ID to delegate to (e.g. `"quant_analyst"`).
    pub persona_id: String,
    /// The question or task to pass to the persona.
    pub message: String,
    /// Optional depth hint: `"basic"`, `"detailed"`, or `"expert"`.
    pub depth: Option<RequestedDepth>,
}

impl Tool for DelegateToPersona {
    const NAME: &'static str = "delegate_to_persona";
    type Error = crate::agent::ToolError;
    type Args = DelegateArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let personas = self
            .registry
            .list()
            .into_iter()
            .map(|(id, desc)| format!("- `{id}`: {desc}"))
            .collect::<Vec<_>>()
            .join("\n");

        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Delegate a question to a specialised sub-agent persona and return its answer.\n\n\
                 Available personas:\n{personas}\n\n\
                 Use this when the question falls squarely in a specialist's domain."
            ),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["persona_id", "message"],
                "properties": {
                    "persona_id": {
                        "type": "string",
                        "description": "ID of the persona to delegate to"
                    },
                    "message": {
                        "type": "string",
                        "description": "The question or task for the persona"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["basic", "detailed", "expert"],
                        "description": "Complexity level for the persona (optional)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.run(args).await.map_err(crate::agent::ToolError::new)
    }
}

impl DelegateToPersona {
    async fn run(&self, args: DelegateArgs) -> Result<serde_json::Value> {
        let persona = self
            .registry
            .get(&args.persona_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown persona: {}", args.persona_id))?;

        let ctx = DisclosureContext {
            turn_count: 0,
            requested_depth: args.depth,
        };

        let agent = PersonaAgent::new(
            persona,
            self.store.clone(),
            self.client.clone(),
            &self.model,
        );

        let reply = agent.chat(&args.message, vec![], &ctx).await?;
        Ok(serde_json::json!({
            "persona": args.persona_id,
            "reply": reply,
        }))
    }
}
