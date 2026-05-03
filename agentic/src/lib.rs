//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind Agentic Layer
//!
//! Two responsibilities:
//!
//! 1. **MCP server** — exposes the platform as a Model Context Protocol endpoint
//!    (via `rmcp`) so Claude or any MCP-compatible client can query signals,
//!    instruments, portfolio state, and trigger strategy runs.
//!
//! 2. **Opportunity analysis assistant** — uses an LLM backend (Anthropic API or
//!    a local OpenAI-compatible inference server) to produce natural-language
//!    briefs on trade candidates, instruments, and macro context.
//!    Both backends are optional; the platform functions fully without them.
//!
//! TODO: Phase 7 — implement MCP server and LlmClient.
