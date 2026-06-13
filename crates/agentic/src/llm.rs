//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! LLM client abstraction — Anthropic API and local OpenAI-compatible backends.
//!
//! # Configuration (environment variables)
//!
//! | Variable              | Effect                                                         |
//! |-----------------------|----------------------------------------------------------------|
//! | `ANTHROPIC_API_KEY`   | Enables the Anthropic cloud backend                           |
//! | `LOCAL_LLM_BASE_URL`  | Base URL of a local OpenAI-compatible server (e.g. Ollama)    |
//! | `LOCAL_LLM_MODEL`     | Model name for the local server (default: `llama3`)           |
//! | `LLM_PROVIDER`        | Force `cloud` or `local` when both are configured             |
//!
//! If neither `ANTHROPIC_API_KEY` nor `LOCAL_LLM_BASE_URL` is set, `LlmClientConfig::build()`
//! returns `None` and all analysis features disable gracefully.

use anyhow::Result;
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde_json::json;
use tracing::debug;

// ── Public trait ──────────────────────────────────────────────────────────────

/// A simple chat-completion interface shared by all LLM backends.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a system + user prompt pair and return the assistant's reply as a String.
    async fn complete(&self, system: &str, user: &str) -> Result<String>;

    /// Provider identifier for logging.
    fn provider_name(&self) -> &str;
}

// ── Config / builder ──────────────────────────────────────────────────────────

/// Which backend to prefer when both are configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmProvider {
    Cloud,
    Local,
}

/// Reads environment variables and constructs the appropriate [`LlmClient`],
/// or returns `None` if no backend is configured.
#[derive(Debug, Default)]
pub struct LlmClientConfig;

impl LlmClientConfig {
    /// Read `ANTHROPIC_API_KEY`, `LOCAL_LLM_BASE_URL`, `LOCAL_LLM_MODEL`, and
    /// `LLM_PROVIDER` from the environment and return the appropriate client.
    ///
    /// Returns `None` if neither backend is configured; the platform continues
    /// operating normally — analysis features are simply skipped.
    pub fn from_env() -> Option<Box<dyn LlmClient>> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let local_url = std::env::var("LOCAL_LLM_BASE_URL").ok();
        let local_model =
            std::env::var("LOCAL_LLM_MODEL").unwrap_or_else(|_| LOCAL_DEFAULT_MODEL.to_string());
        let force_provider =
            std::env::var("LLM_PROVIDER")
                .ok()
                .map(|s| match s.to_lowercase().as_str() {
                    "local" => LlmProvider::Local,
                    _ => LlmProvider::Cloud,
                });

        match (api_key, local_url, force_provider) {
            // Both configured — respect LLM_PROVIDER, default to cloud
            (Some(_), Some(url), Some(LlmProvider::Local)) => {
                tracing::info!("LLM backend: local (forced) at {url}");
                Some(Box::new(LocalBackend::new(url, local_model)))
            }
            (Some(key), Some(_), _) => {
                tracing::info!("LLM backend: Anthropic cloud (preferred)");
                Some(Box::new(AnthropicBackend::new(key)))
            }
            // Only cloud
            (Some(key), None, _) => {
                tracing::info!("LLM backend: Anthropic cloud");
                Some(Box::new(AnthropicBackend::new(key)))
            }
            // Only local
            (None, Some(url), _) => {
                tracing::info!("LLM backend: local at {url}");
                Some(Box::new(LocalBackend::new(url, local_model)))
            }
            // Neither configured
            (None, None, _) => {
                tracing::debug!("No LLM backend configured — analysis features disabled");
                None
            }
        }
    }
}

// ── Anthropic backend ─────────────────────────────────────────────────────────

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-haiku-4-5";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

/// Calls the Anthropic Messages API.
pub struct AnthropicBackend {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicBackend {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, ANTHROPIC_DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicBackend {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}]
        });

        debug!(model = %self.model, "Calling Anthropic API");

        let resp = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let text = resp["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected Anthropic response shape: {resp}"))?
            .to_string();

        Ok(text)
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }
}

// ── Local OpenAI-compatible backend ───────────────────────────────────────────

const LOCAL_DEFAULT_MODEL: &str = "llama3";

/// Calls any OpenAI-compatible `/v1/chat/completions` endpoint (Ollama, llama.cpp, etc.).
pub struct LocalBackend {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl LocalBackend {
    pub fn new(base_url: String, model: String) -> Self {
        let base_url = base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        Self {
            client: reqwest::Client::new(),
            base_url,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for LocalBackend {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user",   "content": user}
            ]
        });

        debug!(model = %self.model, url = %url, "Calling local LLM");

        let resp = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let text = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected local LLM response shape: {resp}"))?
            .to_string();

        Ok(text)
    }

    fn provider_name(&self) -> &str {
        "local"
    }
}
