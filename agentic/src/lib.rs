//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind Agentic Layer — Phase 7
//!
//! Two responsibilities:
//!
//! 1. **MCP server** (`mcp` module) — exposes the platform as a Model Context Protocol
//!    endpoint (via `rmcp`) so Claude or any MCP-compatible client can query signals,
//!    instruments, portfolio state, backtest results, and trigger strategy runs.
//!    Binds on a configurable port (default: 8081).
//!
//! 2. **Opportunity analysis assistant** (`llm` + `analysis` modules) — uses an LLM
//!    backend (Anthropic API or any local OpenAI-compatible inference server) to produce
//!    natural-language briefs on trade candidates, instruments, and macro context.
//!    Both backends are fully optional; the rest of the platform works without them.
//!
//! # Quick start
//!
//! ```ignore
//! // Start the MCP server (blocks until shutdown)
//! let store = DataStore::connect(db_url, duck_path).await?;
//! economind_agentic::mcp::serve(store, 8081).await?;
//!
//! // Or run a one-shot analysis
//! if let Some(llm) = economind_agentic::llm::LlmClientConfig::from_env().build() {
//!     let brief = economind_agentic::analysis::analyze_instrument(&store, &*llm, "AAPL").await?;
//!     println!("{brief}");
//! }
//! ```

pub mod analysis;
pub mod llm;
pub mod mcp;
