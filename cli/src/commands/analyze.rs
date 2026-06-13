//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `economind analyze` — LLM-powered analysis of signals and instruments.
//!
//! # Usage
//! ```text
//! economind analyze signal     <signal-uuid>
//! economind analyze instrument <symbol>
//! economind analyze macro
//! ```
//!
//! Requires `ANTHROPIC_API_KEY` or `LOCAL_LLM_BASE_URL` to be set.
//! Prints an error and exits gracefully if no LLM backend is configured.

use anyhow::Context;
use clap::{Args, Subcommand};
use economind_db::DataStore;
use uuid::Uuid;

// ── Clap types ────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct AnalyzeArgs {
    #[command(subcommand)]
    pub command: AnalyzeCommand,
}

#[derive(Subcommand)]
pub enum AnalyzeCommand {
    /// Produce a natural-language rationale review for a trade signal.
    Signal(AnalyzeSignalArgs),

    /// Produce a natural-language brief for an instrument.
    Instrument(AnalyzeInstrumentArgs),

    /// Summarise the current macro environment.
    Macro,
}

#[derive(Args)]
pub struct AnalyzeSignalArgs {
    /// Signal UUID.
    pub signal_id: Uuid,
}

#[derive(Args)]
pub struct AnalyzeInstrumentArgs {
    /// Ticker symbol (e.g. AAPL).
    pub symbol: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(
    args: AnalyzeArgs,
    database_url: &str,
    duckdb_path: &str,
) -> anyhow::Result<()> {
    let store = DataStore::connect(database_url, duckdb_path)
        .await
        .context("Failed to connect to DataStore")?;

    let llm = economind_agentic::llm::LlmClientConfig::from_env();

    if llm.is_none() {
        eprintln!(
            "No LLM backend configured.\n\
             Set ANTHROPIC_API_KEY (Anthropic cloud) or\n\
             LOCAL_LLM_BASE_URL (local OpenAI-compatible server, e.g. Ollama)."
        );
        std::process::exit(1);
    }

    let llm_ref = llm.as_deref();

    match args.command {
        AnalyzeCommand::Signal(a) => {
            let brief = economind_agentic::analysis::analyze_signal(&store, llm_ref, a.signal_id)
                .await
                .context("Analysis failed")?
                .expect("LLM is Some — brief must be Some");
            println!("{brief}");
        }

        AnalyzeCommand::Instrument(a) => {
            let brief =
                economind_agentic::analysis::analyze_instrument(&store, llm_ref, &a.symbol)
                    .await
                    .context("Analysis failed")?
                    .expect("LLM is Some — brief must be Some");
            println!("{brief}");
        }

        AnalyzeCommand::Macro => {
            let brief = economind_agentic::analysis::analyze_macro(&store, llm_ref)
                .await
                .context("Analysis failed")?
                .expect("LLM is Some — brief must be Some");
            println!("{brief}");
        }
    }

    Ok(())
}
