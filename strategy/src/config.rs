//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Strategy configuration types.
//!
//! A `StrategyConfig` describes a named, versioned strategy: which composition
//! mode to use, which plugins to wire in, and the parameter map that plugins
//! read at runtime.  Configs are persisted in `strategy.configs` (§1.B.3).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ── Composition mode ──────────────────────────────────────────────────────────

/// How multiple strategy stacks are combined into a single signal set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompositionMode {
    /// Phase 2: single Identifier → Timer(s) → Sizer pipeline.
    Pipeline,
    /// Phase 8: majority-vote across multiple stacks.
    Voting,
    /// Phase 8: weighted ensemble of multiple stacks.
    Ensemble,
}

impl std::fmt::Display for CompositionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompositionMode::Pipeline => write!(f, "pipeline"),
            CompositionMode::Voting => write!(f, "voting"),
            CompositionMode::Ensemble => write!(f, "ensemble"),
        }
    }
}

// ── Plugin spec ───────────────────────────────────────────────────────────────

/// Identifies a plugin by role + name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSpec {
    /// Plugin role: "identifier", "timer", or "sizer".
    pub role: String,
    /// Plugin name (matches `Plugin::name()`).
    pub name: String,
}

// ── StrategyConfig ────────────────────────────────────────────────────────────

/// A versioned, named strategy configuration.
///
/// Each config stores the composition mode, the ordered list of plugin specs,
/// and a flat key-value parameter map that plugins read via `ctx.parameters`.
///
/// The `version` field is incremented on every parameter change so historical
/// runs can always be replayed with the exact parameters in effect at the time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub composition: CompositionMode,
    /// Ordered plugin specs.  For `Pipeline` mode: first identifier(s), then
    /// timer(s), then exactly one sizer.
    pub plugins: Vec<PluginSpec>,
    /// Flat key-value parameters passed to all plugins via `StrategyContext`.
    pub parameters: HashMap<String, String>,
    pub enabled: bool,
    /// Incremented on every parameter change.
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyConfig {
    /// Create a new in-memory config (not yet persisted).
    pub fn new(
        name: impl Into<String>,
        composition: CompositionMode,
        plugins: Vec<PluginSpec>,
        parameters: HashMap<String, String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            composition,
            plugins,
            parameters,
            enabled: true,
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }
}
