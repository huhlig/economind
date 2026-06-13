//! `economind universe` — manage the tracked instrument universe.
//!
//! # Usage
//! ```text
//! economind universe list
//! economind universe add  <SYMBOL> [<SYMBOL>...]
//! economind universe load --file <path>   (default: universe.csv in cwd)
//! ```

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use economind_core::model::Symbol;
use economind_db::{DataStore, MetadataStorage};
use futures::StreamExt;

// ── Clap types ────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct UniverseArgs {
    #[command(subcommand)]
    pub command: UniverseCommand,
}

#[derive(Subcommand)]
pub enum UniverseCommand {
    /// Print every symbol in the universe.
    List,

    /// Add one or more symbols to the universe.
    Add {
        /// Ticker symbols to add (e.g. AAPL MSFT GOOG).
        #[arg(required = true)]
        symbols: Vec<String>,
    },

    /// Bulk-load symbols from a CSV file (one `symbol` column, header required).
    Load {
        /// Path to the CSV file. Defaults to `universe.csv` in the current directory.
        #[arg(long, short, default_value = "universe.csv")]
        file: String,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(args: UniverseArgs, duckdb_path: &str) -> Result<()> {
    let store = DataStore::open(duckdb_path).context("opening database")?;

    match args.command {
        UniverseCommand::List => list(&store).await,
        UniverseCommand::Add { symbols } => add(&store, symbols).await,
        UniverseCommand::Load { file } => load(&store, &file).await,
    }
}

// ── Subcommand implementations ────────────────────────────────────────────────

async fn list(store: &DataStore) -> Result<()> {
    let mut stream = store.list_tickers().await.context("listing universe")?;
    let mut count = 0usize;
    while let Some(sym) = stream.next().await {
        println!("{}", sym.as_str());
        count += 1;
    }
    if count == 0 {
        println!("Universe is empty. Use `economind universe load` to add symbols.");
    } else {
        eprintln!("{} symbol(s)", count);
    }
    Ok(())
}

async fn add(store: &DataStore, symbols: Vec<String>) -> Result<()> {
    let mut added = 0usize;
    for raw in symbols {
        let sym = Symbol::new(&raw.to_uppercase());
        store
            .upsert_ticker(&sym)
            .await
            .with_context(|| format!("upserting {}", raw))?;
        println!("added {}", sym.as_str());
        added += 1;
    }
    eprintln!("{} symbol(s) added", added);
    Ok(())
}

async fn load(store: &DataStore, path: &str) -> Result<()> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading universe file: {path}"))?;

    let mut reader = csv::Reader::from_reader(content.as_bytes());

    // Find the `symbol` column (case-insensitive).
    let headers = reader.headers().context("reading CSV headers")?.clone();
    let col = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("symbol"))
        .context("CSV must have a `symbol` column")?;

    let mut added = 0usize;
    let mut skipped = 0usize;
    for result in reader.records() {
        let record = result.context("reading CSV record")?;
        let raw = match record.get(col) {
            Some(s) if !s.trim().is_empty() => s.trim().to_uppercase(),
            _ => {
                skipped += 1;
                continue;
            }
        };
        let sym = Symbol::new(&raw);
        store
            .upsert_ticker(&sym)
            .await
            .with_context(|| format!("upserting {raw}"))?;
        added += 1;
    }

    println!("Loaded {added} symbol(s) from {path} ({skipped} blank rows skipped)");
    Ok(())
}
