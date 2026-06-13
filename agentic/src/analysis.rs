//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Opportunity analysis assistant — builds context from DataStore and calls an LLM
//! to produce natural-language briefs on instruments, signals, and macro conditions.
//!
//! All public functions return `Ok(None)` when `llm` is `None` so callers can
//! gracefully skip analysis rather than hard-failing.

use anyhow::Result;
use chrono::Utc;
use economind_db::{DataStore, MacroStorage, MetadataStorage, StrategyStorage};
use uuid::Uuid;

use crate::llm::LlmClient;
use crate::mcp::collect_bars;

// ── FRED series labels ────────────────────────────────────────────────────────

const MACRO_SERIES: &[(&str, &str)] = &[
    ("DGS10",     "10-Year Treasury Yield (%)"),
    ("T10Y2Y",    "10Y-2Y Yield Curve Spread (%)"),
    ("CPIAUCSL",  "CPI (YoY % change proxy)"),
    ("UNRATE",    "Unemployment Rate (%)"),
    ("VIXCLS",    "VIX Volatility Index"),
    ("M2SL",      "M2 Money Supply (billions USD)"),
];

// ── System prompt ─────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "\
You are a quantitative analyst assistant for a low-frequency trading platform \
(trade horizons 1 day to 1 month). Your analysis is concise, data-driven, and \
actionable. Avoid generic disclaimers. Focus on what the data says.";

// ── Public entry points ───────────────────────────────────────────────────────

/// Produce a natural-language brief for a trade signal.
///
/// Pulls instrument data and macro context from `store`, constructs a prompt,
/// and calls the LLM. Returns `Ok(None)` if `llm` is `None`.
pub async fn analyze_signal(
    store: &DataStore,
    llm: Option<&dyn LlmClient>,
    signal_id: Uuid,
) -> Result<Option<String>> {
    let llm = match llm {
        Some(l) => l,
        None => return Ok(None),
    };

    let signal = store
        .get_strategy_signal(signal_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Signal {signal_id} not found"))?;

    let macro_ctx  = build_macro_context(store).await;
    let ticker_ctx = build_ticker_context(store, &signal.symbol).await;

    let user = format!(
        "## Trade Signal\n\
         - Symbol: {symbol}\n\
         - Direction: {direction}\n\
         - Identifier score: {id_score:.3}\n\
         - Timing score: {ti_score:.3}\n\
         - Position size: {shares} shares / {notional} notional ({pct}% of portfolio)\n\
         - Strategy rationale: {rationale}\n\
         - Emitted at: {emitted_at}\n\
         \n\
         {ticker_ctx}\
         \n\
         {macro_ctx}\
         \n\
         In 3–5 sentences, assess whether this signal makes intuitive sense given \
         the instrument data and macro environment. Note any risks or confirming factors.",
        symbol     = signal.symbol,
        direction  = signal.direction,
        id_score   = signal.identifier_score,
        ti_score   = signal.timing_score,
        shares     = signal.position_shares.map(|d| d.to_string()).unwrap_or_default(),
        notional   = signal.position_notional.map(|d| d.to_string()).unwrap_or_default(),
        pct        = signal.portfolio_fraction
                         .map(|d| format!("{:.1}", d * rust_decimal::Decimal::from(100)))
                         .unwrap_or_default(),
        rationale  = signal.rationale.as_deref().unwrap_or("(none)"),
        emitted_at = signal.emitted_at.to_rfc3339(),
        ticker_ctx = ticker_ctx,
        macro_ctx  = macro_ctx,
    );

    let brief = llm.complete(SYSTEM_PROMPT, &user).await?;
    Ok(Some(brief))
}

/// Produce a natural-language brief for an instrument (current state only).
///
/// Returns `Ok(None)` if `llm` is `None`.
pub async fn analyze_instrument(
    store: &DataStore,
    llm: Option<&dyn LlmClient>,
    symbol: &str,
) -> Result<Option<String>> {
    let llm = match llm {
        Some(l) => l,
        None => return Ok(None),
    };

    let ticker_ctx = build_ticker_context(store, symbol).await;
    let macro_ctx  = build_macro_context(store).await;

    let user = format!(
        "## Instrument: {symbol}\n\
         \n\
         {ticker_ctx}\
         \n\
         {macro_ctx}\
         \n\
         In 3–5 sentences, summarise the current state of this instrument and \
         whether it looks attractive for a 1-day to 1-month long position given \
         the macro environment.",
        symbol     = symbol,
        ticker_ctx = ticker_ctx,
        macro_ctx  = macro_ctx,
    );

    let brief = llm.complete(SYSTEM_PROMPT, &user).await?;
    Ok(Some(brief))
}

/// Produce a macro environment summary.
///
/// Returns `Ok(None)` if `llm` is `None`.
pub async fn analyze_macro(
    store: &DataStore,
    llm: Option<&dyn LlmClient>,
) -> Result<Option<String>> {
    let llm = match llm {
        Some(l) => l,
        None => return Ok(None),
    };

    let macro_ctx = build_macro_context(store).await;

    let user = format!(
        "{macro_ctx}\n\n\
         In 3–5 sentences, summarise what the current macro environment implies \
         for equity long positions over a 1-day to 1-month horizon.",
        macro_ctx = macro_ctx,
    );

    let brief = llm.complete(SYSTEM_PROMPT, &user).await?;
    Ok(Some(brief))
}

// ── Context builders ──────────────────────────────────────────────────────────

/// Build a short instrument context block from DataStore metadata and recent bars.
async fn build_ticker_context(store: &DataStore, symbol: &str) -> String {
    use economind_core::model::Symbol;

    let sym = Symbol::new(symbol);
    let mut lines = Vec::new();

    lines.push(format!("## Instrument Context: {symbol}"));

    // Ticker metadata
    if let Ok(Some(t)) = store.get_ticker(&sym).await {
        if let Some(name) = &t.name {
            lines.push(format!("- Name: {name}"));
        }
        if let Some(sector) = &t.sector {
            lines.push(format!("- Sector: {sector:?}"));
        }
        if let Some(industry) = &t.industry {
            lines.push(format!("- Industry: {industry:?}"));
        }
        if let Some(mc) = t.marketcap {
            lines.push(format!("- Market cap: ${mc}"));
        }
    }

    // Last 10 daily bars
    let to   = Utc::now().date_naive();
    let from = to - chrono::Duration::days(20);
    if let Ok(bars) = collect_bars(store, &sym, from, to, 10).await {
        if !bars.is_empty() {
            lines.push("- Recent daily bars (date, close, volume):".to_string());
            for bar in &bars {
                lines.push(format!(
                    "  {}  close={}  vol={}",
                    bar.date, bar.close, bar.volume
                ));
            }
        }
    }

    if lines.len() == 1 {
        lines.push("  (no data available)".to_string());
    }

    lines.join("\n")
}

/// Build a macro environment context block from the latest FRED values.
async fn build_macro_context(store: &DataStore) -> String {
    let ids: Vec<&str> = MACRO_SERIES.iter().map(|(id, _)| *id).collect();
    let mut lines = vec!["## Current Macro Environment".to_string()];

    match store.get_latest_macro_values(&ids).await {
        Ok(points) if !points.is_empty() => {
            for (series_id, label) in MACRO_SERIES {
                if let Some(p) = points.iter().find(|p| &p.series_id == series_id) {
                    let val = p.value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "N/A".to_string());
                    lines.push(format!("- {label}: {val}  (as of {})", p.date));
                }
            }
        }
        _ => {
            lines.push(
                "  (macro data unavailable — run `economind ingest macro` first)".to_string(),
            );
        }
    }

    lines.join("\n")
}
