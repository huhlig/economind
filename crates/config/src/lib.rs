//! Economind platform configuration.
//!
//! Configuration is layered:
//!   1. `economind.toml` — non-secret settings (checked in, safe to commit)
//!   2. Environment variables — secrets and deployment overrides (always win)
//!
//! Secrets (API keys, database passwords) must come from environment variables.
//! They are never stored in `economind.toml`.
//!
//! # Usage
//! ```ignore
//! let cfg = EconomindConfig::load()?;
//! println!("Binding to {}", cfg.server.bind);
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Top-level config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomindConfig {
    #[serde(default)]
    pub database: DatabaseConfig,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub schedule: ScheduleConfig,

    #[serde(default)]
    pub ingest: IngestConfig,

    #[serde(default)]
    pub llm: LlmConfig,

    #[serde(default)]
    pub risk: RiskConfig,

    #[serde(default)]
    pub notifications: NotificationsConfig,
}

// ── [database] ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// DuckDB file path. Use `:memory:` for ephemeral in-process DB.
    /// Override via DUCKDB_PATH env var (takes precedence).
    pub duckdb_path: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            duckdb_path: ":memory:".to_string(),
        }
    }
}

impl DatabaseConfig {
    /// Return the effective DuckDB path (env var wins over toml).
    pub fn effective_duckdb_path(&self) -> String {
        std::env::var("DUCKDB_PATH").unwrap_or_else(|_| self.duckdb_path.clone())
    }
}

// ── [server] ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Address and port to bind the HTTP server on.
    /// Override via ECONOMIND_BIND env var.
    pub bind: String,

    /// Port for the MCP server (separate from REST API).
    /// Override via ECONOMIND_MCP_PORT env var.
    pub mcp_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8080".to_string(),
            mcp_port: 8081,
        }
    }
}

impl ServerConfig {
    pub fn effective_bind(&self) -> String {
        std::env::var("ECONOMIND_BIND").unwrap_or_else(|_| self.bind.clone())
    }

    pub fn effective_mcp_port(&self) -> u16 {
        std::env::var("ECONOMIND_MCP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.mcp_port)
    }
}

// ── [schedule] ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// Whether the scheduler is enabled at all.
    pub enabled: bool,

    /// UTC time (HH:MM) for daily bar ingestion. Default: 22:00 (5 PM ET).
    pub bars_utc: String,

    /// UTC time (HH:MM) for daily macro refresh. Default: 23:00 (6 PM ET).
    pub macro_utc: String,

    /// UTC time (HH:MM) for weekly fundamentals refresh (runs Sunday). Default: 23:00.
    pub fundamentals_utc: String,

    /// UTC time (HH:MM) for daily strategy run. Default: 23:30 (6:30 PM ET).
    pub strategy_utc: String,

    /// Number of days of bars to fetch on each nightly bar ingestion run.
    pub bars_lookback_days: u32,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bars_utc: "22:00".to_string(),
            macro_utc: "23:00".to_string(),
            fundamentals_utc: "23:00".to_string(),
            strategy_utc: "23:30".to_string(),
            bars_lookback_days: 5,
        }
    }
}

// ── [ingest] ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestConfig {
    /// Concurrent symbol downloads during bar ingestion.
    pub bar_concurrency: usize,

    /// Default lookback window (days) for initial bar backfill.
    pub bar_backfill_days: u32,

    /// FRED series IDs to fetch. Leave empty to use the built-in defaults.
    pub fred_series: Vec<String>,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            bar_concurrency: 4,
            bar_backfill_days: 365,
            fred_series: vec![],
        }
    }
}

// ── [llm] ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Which provider to use: "anthropic" | "local" | "auto".
    /// "auto" picks Anthropic if ANTHROPIC_API_KEY is set, local otherwise.
    pub provider: String,

    /// Default Claude model for Anthropic backend.
    pub anthropic_model: String,

    /// Base URL for the local OpenAI-compatible inference server.
    pub local_base_url: String,

    /// Model name for the local backend.
    pub local_model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "auto".to_string(),
            anthropic_model: "claude-haiku-4-5".to_string(),
            local_base_url: "http://localhost:11434/v1".to_string(),
            local_model: "llama3".to_string(),
        }
    }
}

// ── [risk] ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Halt all new order execution when portfolio drawdown exceeds this fraction (0.0–1.0).
    /// Example: 0.20 = halt when down >20% from equity peak.
    pub max_drawdown_pct: f64,

    /// Maximum fraction of portfolio value any single position may represent.
    /// Example: 0.10 = no position can exceed 10% of portfolio.
    pub max_position_pct: f64,

    /// Maximum number of concurrent open positions.
    pub max_open_positions: usize,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_drawdown_pct: 0.20,
            max_position_pct: 0.10,
            max_open_positions: 20,
        }
    }
}

// ── [notifications] ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationsConfig {
    /// Discord or Slack incoming webhook URL.
    /// Leave empty to disable notifications.
    pub webhook_url: Option<String>,

    /// Send a notification when a signal is emitted.
    pub on_signal: bool,

    /// Send a notification when the strategy run completes.
    pub on_run_complete: bool,

    /// Send a notification when an order is submitted to the broker.
    pub on_order: bool,

    /// Send a notification when an error occurs.
    pub on_error: bool,
}

// ── Loading ───────────────────────────────────────────────────────────────────

impl EconomindConfig {
    /// Load configuration from `economind.toml` in the current directory.
    ///
    /// Missing file is not an error — defaults are used.
    /// Malformed TOML is an error.
    pub fn load() -> Result<Self> {
        Self::load_from("economind.toml")
    }

    /// Load configuration from the given path.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;

        toml::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))
    }
}
