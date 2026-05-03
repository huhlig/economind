# Economind Implementation Plan

> **This document is the authoritative source of work for the Economind platform.**
> The architecture document (`ARCHITECTURE.md`) is a collaborative reference, not a specification.
> Where this plan and the architecture document conflict, this plan takes precedence.

*Last updated: May 2026 — Phase 5 complete*

## Progress

| Task | Status | Notes |
|------|--------|-------|
| **1.A** Crate restructure | ✅ Complete | All 16 crates created; old crates removed from workspace; zero stale imports |
| **1.B** PostgreSQL schema & migrations | ✅ Complete | 3 migration files; `PostgresStorage` fully implements all storage traits |
| **1.C** DuckDB storage layer | ✅ Complete | `DuckDatabase` fully implemented; `DataStore` facade + PG→Duck sync implemented |
| **1.D** Smoke test | ✅ Complete | 10 DuckDB round-trip tests + 3 PostgreSQL tests gated on `DATABASE_URL` |
| **2.A** Core trait finalization | ✅ Complete | `StrategyConfig`, `StrategyStorage`/`MacroStorage`/`PortfolioStorage` traits + PG impls added |
| **2.B** Pipeline composition engine | ✅ Complete | `PipelineRunner` + `PipelineRunnerBuilder`; `run_strategy` orchestrator with DB persistence |
| **2.C** First strategy plugins | ✅ Complete | `strategy-momentum` (Identifier), `strategy-mean-reversion` (Timer), `strategy-atr-sizer` (Sizer) |
| **2.D** CLI wiring (strategy) | ✅ Complete | `economind run --config <uuid>` and `economind signals` subcommands |
| **3.A** Yahoo Finance connector | ✅ Complete | `YahooFinanceConnector`: daily bars (v8 chart), metadata (v10 quote summary), bulk fetch with concurrency |
| **3.B** FRED macro connector | ✅ Complete | `FredConnector`: fetches DGS10/T10Y2Y/CPIAUCSL/UNRATE/VIXCLS/M2SL; `MacroStorage::upsert_macro_series` added to trait + PG impl + DataStore |
| **3.C** SEC EDGAR connector | ✅ Complete | `EdgarConnector`: XBRL company facts → IS/BS/CF; CIK lookup cache from SEC tickers JSON |
| **3.D** SimFin connector | ✅ Complete | `SimFinConnector`: v3 columnar response parser → IS/BS/CF |
| **3.E** Ingestion orchestration | ✅ Complete | `DataFeedManager` with `run_bars`/`run_macro`/`run_fundamentals`/`run_job_by_name`; `economind ingest` CLI subcommand |
| **4.A** Backtest simulation loop | ✅ Complete | `BacktestRunner` + `SimPortfolio`; no-lookahead context, slippage/commission, max-hold-days forced exit, end-of-run close-all |
| **4.B** Performance metrics | ✅ Complete | `PerformanceMetrics`: CAGR, Sharpe, Sortino, max drawdown + duration, win rate, profit factor, expectancy, avg hold, largest win/loss |
| **4.C** CLI wiring (backtest) | ✅ Complete | `economind backtest run` + `economind backtest list`; formatted summary table; `BacktestStorage` trait + PG impl + DataStore facade |
| **5.A** Axum server scaffold | ✅ Complete | `AppState` with `DataStore` + `EventBus`; `require_api_key` middleware; `health` endpoint |
| **5.B** REST endpoints | ✅ Complete | `instruments`, `signals`, `positions`, `strategy/configs`, `strategy/run`, `backtest`, `data/bars`, `data/fundamentals`, `data/macro` |
| **5.C** GraphQL | ✅ Complete | `async-graphql` schema; `QueryRoot` (instruments, signals, portfolio, strategy configs, backtest runs); `MutationRoot` (add/remove instrument, update config, trigger run); GraphiQL at `/graphiql` |
| **5.D** WebSocket | ✅ Complete | `GET /ws/signals`; `ServerEvent` enum with 7 event types; broadcast via `EventBus`; API-key auth on upgrade |

---

## Table of Contents

1. [Crate Structure Decisions](#1-crate-structure-decisions)
2. [Phase Overview](#2-phase-overview)
3. [Phase 1 — Foundation](#3-phase-1--foundation)
4. [Phase 2 — Strategy Engine](#4-phase-2--strategy-engine)
5. [Phase 3 — Data Coverage](#5-phase-3--data-coverage)
6. [Phase 4 — Backtest Engine](#6-phase-4--backtest-engine)
7. [Phase 5 — API Layer](#7-phase-5--api-layer)
8. [Phase 6 — Web Dashboard](#8-phase-6--web-dashboard)
9. [Phase 7 — Agentic Layer](#9-phase-7--agentic-layer)
10. [Phase 8 — Composition Modes & Optimization](#10-phase-8--composition-modes--optimization)
11. [Effort Key](#11-effort-key)
12. [Open Decisions](#12-open-decisions)

---

## 1. Crate Structure Decisions

The prototype's five crates are evaluated individually below. Each decision includes a rationale and the resulting action.

### 1.1 `economind-datamodel` → **rename + split**

**Current role:** Core data types (OHLCV, fundamentals, corporate actions, news, patterns, factor snapshots) plus the storage layer (PostgreSQL partial, DuckDB stubbed).

**Problem:** Mixing domain types with database access violates the principle that `core` should have zero runtime dependencies. The storage traits and implementations need to live in a dedicated `db` crate.

**Decision:** Split into two crates:
- **`economind-core`** — all types, traits (`Identifier`, `Timer`, `Sizer`, `DataStore`), and error definitions. Zero runtime dependencies beyond `serde`, `chrono`, and `rust_decimal`. This is what the prototype's `model/` module becomes.
- **`economind-db`** — all database access: PostgreSQL (sqlx), DuckDB (duckdb-rs), migrations, connection pooling. Imports `economind-core` for types. This is what the prototype's `storage/` module becomes.

**Migration work:** Move `src/model/` → `economind-core/src/`, move `src/storage/` → `economind-db/src/`, update all imports.

---

### 1.2 `economind-algorithms` → **rename**

**Current role:** Pattern detection engine (14 patterns), ATR, volume SMA, breakout detection, target projection, sliding window scanning. Screener stub.

**Problem:** The name `algorithms` is vague. The architecture calls this `economind-indicators`, which better describes pure-function signal computation.

**Decision:** **Rename to `economind-indicators`**. No structural changes needed — the existing implementation is mature and maps well to the planned role. The screener stub should be removed or relocated to a strategy plugin.

**Migration work:** `Cargo.toml` rename, update workspace member entry and any downstream imports. Low risk.

---

### 1.3 `economind-datafeed` → **rename**

**Current role:** Data provider traits, `RateLimitedClient`, partial Kibot connector, stubs for Tiingo, RReichel, and several archive providers.

**Problem:** The name `datafeed` implies live streaming only. The architecture calls this `economind-ingest` to cover scheduled batch ingestion as well as streaming feeds.

**Decision:** **Rename to `economind-ingest`**. Keep all existing trait definitions and the `RateLimitedClient`. Remove the archive provider stubs (Finnhub, FMP Cloud, MarketStack, StockData, Polygon) — these are not in the current data source plan. Kibot is also not in the free-data-first plan; retain the code but deprioritize it.

**Migration work:** `Cargo.toml` rename, trim archive stubs, update imports.

---

### 1.4 `economind-agentic` → **repurpose and expand**

**Current role:** Empty stub.

**Decision:** **Keep as `economind-agentic`** — this is the right name for the layer. It serves two purposes not present in the original architecture document:

1. **Opportunity analysis assistant** — an agentic layer that can examine signals, news, fundamentals, and macro data to produce natural-language summaries and second opinions on trade candidates. Uses an LLM (via Anthropic API or local model) to digest information and surface non-obvious patterns.

2. **MCP server endpoint** — exposes the platform as an MCP (Model Context Protocol) server so that Claude (or any MCP-compatible client) can query the system: ask about current signals, portfolio state, backtest results, instrument data, or trigger strategy runs via natural language.

This crate will depend on `economind-core` and `economind-db` for data access, and will implement the `rmcp` Rust MCP SDK to serve the MCP protocol. It is intentionally decoupled from `economind-api` so the MCP surface can be used independently of the REST/GraphQL layer.

**Migration work:** Implement from scratch. No existing code to migrate.

---

### 1.5 `economind-server` → **rename**

**Current role:** `println!("Hello, world!")` stub.

**Decision:** **Rename to `economind-api`**. Build Axum REST + GraphQL + WebSocket server here. The binary entry point for the full platform (serves API, static dashboard assets, and optionally the MCP server on a separate port).

**Migration work:** Replace stub with full Axum scaffold. Low risk — nothing to preserve.

---

### 1.6 New crates to add

These do not exist in the prototype and must be created:

| New Crate | Purpose |
|-----------|---------|
| `economind-strategy` | Strategy engine: `Identifier`/`Timer`/`Sizer` trait hosting, pipeline composition, run orchestration |
| `economind-backtest` | Historical simulation using DuckDB; performance metrics |
| `economind-cli` | `clap` CLI; thin wrapper delegating to other crates |
| `strategy-momentum` | Identifier plugin: cross-sectional momentum screener |
| `strategy-regime` | Identifier plugin: HMM-based market regime classifier |
| `strategy-mean-reversion` | Timer plugin: Bollinger Bands + Z-score + RSI |
| `strategy-trend-follow` | Timer plugin: multi-timeframe EMA + ADX |
| `strategy-atr-sizer` | Sizer plugin: ATR-based volatility position sizing |
| `strategy-kelly-sizer` | Sizer plugin: Fractional Kelly criterion |

---

### 1.7 Final crate map

```
economind/
├── economind-core          (was: economind-datamodel/src/model/)
├── economind-db            (was: economind-datamodel/src/storage/)
├── economind-indicators    (was: economind-algorithms)
├── economind-ingest        (was: economind-datafeed)
├── economind-strategy      (new)
├── economind-backtest      (new)
├── economind-api           (was: economind-server)
├── economind-cli           (new)
├── economind-agentic       (repurposed; was empty stub)
└── strategies/
    ├── strategy-momentum
    ├── strategy-regime
    ├── strategy-mean-reversion
    ├── strategy-trend-follow
    ├── strategy-atr-sizer
    └── strategy-kelly-sizer
```

---

## 2. Phase Overview

| Phase | Name | Goal | Key Output |
|-------|------|------|-----------|
| **1** | Foundation | Clean, buildable workspace with correct crate structure and complete data layer | All crates compile; PostgreSQL + DuckDB fully working |
| **2** | Strategy Engine | End-to-end signal generation via pipeline composition | First real trade signal produced and logged |
| **3** | Data Coverage | Full free-tier data ingestion (prices, fundamentals, macro) | Platform feeds itself without manual data loading |
| **4** | Backtest Engine | Historical simulation and performance measurement | Backtest run produces Sharpe, drawdown, win rate |
| **5** | API Layer | REST + GraphQL + WebSocket surface | All platform operations accessible via HTTP |
| **6** | Web Dashboard | SvelteKit single-page app | Full UI for signals, portfolio, strategy management |
| **7** | Agentic Layer | MCP server + opportunity analysis assistant | Claude can query and reason about platform data |
| **8** | Composition Modes | Voting and Ensemble modes; weight optimization | Multi-strategy consensus and ensemble signals |

Phases are sequential with soft dependencies. Phase 3 (data) can be started in parallel with Phase 2 (strategy engine) once Phase 1 is complete.

---

## 3. Phase 1 — Foundation

**Goal:** A clean, fully-compiling workspace with the correct crate structure, complete type definitions, and a fully working database layer (both PostgreSQL and DuckDB).

**Exit criteria:**
- All crates in the final crate map compile with no warnings
- PostgreSQL migrations run cleanly and all storage trait methods are implemented
- DuckDB storage layer is fully implemented (no `todo!()` remaining)
- A smoke-test binary can ingest a CSV of OHLCV bars and query them back from both databases

---

### 1.A Crate restructuring

| Task | Detail | Effort |
|------|--------|--------|
| **1.A.1** Create `economind-core` crate | New crate. Move `economind-datamodel/src/model/` contents in. Add `Identifier`, `Timer`, `Sizer`, and `DataStore` trait stubs (bodies can be `todo!()` at this stage — we're just establishing the trait signatures). Keep deps minimal: `serde`, `chrono`, `rust_decimal`, `thiserror`. | M |
| **1.A.2** Create `economind-db` crate | New crate. Move `economind-datamodel/src/storage/` contents in. Depends on `economind-core`. Carries `sqlx`, `duckdb-rs` deps. | S |
| **1.A.3** Rename `economind-algorithms` → `economind-indicators` | Update `Cargo.toml` name field and workspace member entry. Update any imports in other crates. | S |
| **1.A.4** Rename `economind-datafeed` → `economind-ingest` | Update `Cargo.toml` name field. Remove archive provider stubs (alphavantage, finnhub, fmpcloud, marketstack, polygonio, stockdata). Retain `RateLimitedClient`, provider traits, and Kibot partial impl. | S |
| **1.A.5** Rename `economind-server` → `economind-api` | Update name. Replace `main.rs` stub with a minimal Axum hello-world that compiles. | S |
| **1.A.6** Remove `economind-datamodel` | Once 1.A.1 and 1.A.2 are complete, remove the original crate from the workspace and delete its directory. | S |
| **1.A.7** Create `economind-strategy` crate stub | New crate with `lib.rs`. Define `StrategyContext` struct and the `Identifier`, `Timer`, `Sizer` traits (move from core if already defined there, or define here — see open decision). No implementations yet. | S |
| **1.A.8** Create `economind-cli` crate stub | New crate with `main.rs` using clap. Single subcommand `version` that prints the binary version. Delegates to other crates — no business logic of its own. | S |
| **1.A.9** Verify full workspace compiles | Run `cargo build --workspace` and resolve all errors and warnings. | S |

---

### 1.B PostgreSQL schema and migrations

| Task | Detail | Effort |
|------|--------|--------|
| **1.B.1** Audit and migrate existing schema | Review existing `postgres.sql`. The current schema (`settings`, `tickers`, `ticker_stats`, `daily_candle`) uses a flat `economind` namespace. Map these to the planned schema namespaces (`market.*`, `strategy.*`, `portfolio.*`, `backtest.*`, `system.*`). Produce a migration script. | M |
| **1.B.2** Add TimescaleDB hypertable for bars | Convert `market.bars` to a TimescaleDB hypertable partitioned on `time`. Add chunk interval of 1 month. | S |
| **1.B.3** Add strategy and signal tables | Create `strategy.configs` (versioned parameter sets), `strategy.signals` (full signal history), and `strategy.runs` (run metadata). | M |
| **1.B.4** Add portfolio tables | Create `portfolio.positions` (current + historical), `portfolio.trades` (executed trade log). | M |
| **1.B.5** Add backtest tables | Create `backtest.runs` (metadata + metrics) and `backtest.trades` (simulated trade log). | M |
| **1.B.6** Add fundamentals and macro tables | Create `market.fundamentals` (income statement, balance sheet, cash flow keyed by instrument + period), `market.macro_series` (FRED series keyed by series_id + date). | M |
| **1.B.7** Add audit log table | Create `system.audit_log` as an append-only, immutable event log (insert trigger prevents UPDATE/DELETE). | S |
| **1.B.8** Implement all PostgreSQL storage trait methods | Complete the partial `PostgresStorage` impl. Stub methods exist for tickers; implement candles, fundamentals, signals, positions, trades. | L |

---

### 1.C DuckDB storage layer

| Task | Detail | Effort |
|------|--------|--------|
| **1.C.1** Implement DuckDB connection and init | Replace `todo!()` in `DuckDatabase::new()`. On open, run schema creation SQL if tables don't exist. | S |
| **1.C.2** Implement ticker CRUD in DuckDB | `upsert_ticker`, `get_ticker`, `list_tickers` — map `Ticker` struct to DuckDB row via `duckdb-rs`. | S |
| **1.C.3** Implement candle storage in DuckDB | `upsert_candles` (batch insert), `query_candles` (with symbol + date range + interval filters). Use prepared statements. | M |
| **1.C.4** Implement daily candle storage in DuckDB | Same as 1.C.3 but for `daily_candles` table. | S |
| **1.C.5** Implement news storage in DuckDB | `insert_news`, `query_news` with symbol + date range filters. | S |
| **1.C.6** Implement PostgreSQL → DuckDB sync | A `sync_from_postgres(pool, duck)` function that exports bars and fundamentals from PostgreSQL to DuckDB parquet snapshots for analytical queries. Called before each strategy run. | M |
| **1.C.7** Implement DataStore trait | The `DataStore` trait (defined in `economind-core` or `economind-db`) should provide a unified interface that dispatches reads to DuckDB (fast columnar) and writes to PostgreSQL (durable). Implement the trait struct that holds both connections. | M |

---

### 1.D Smoke test

| Task | Detail | Effort |
|------|--------|--------|
| **1.D.1** Write integration smoke test | A test binary (or `#[tokio::test]` integration test) that: (1) connects to a local PostgreSQL instance, (2) inserts 100 daily bars for a dummy instrument, (3) syncs to DuckDB, (4) queries them back from DuckDB. Asserts row counts match. | M |

---

## 4. Phase 2 — Strategy Engine

**Goal:** A working strategy engine that can run a pipeline of pluggable strategies and produce a logged signal for at least one instrument.

**Exit criteria:**
- `Identifier`, `Timer`, `Sizer` traits are fully defined
- `StrategyContext` carries real data from the DataStore
- Pipeline composition mode runs end-to-end
- At least one Identifier, one Timer, and one Sizer plugin are implemented
- Signals are persisted to `strategy.signals`

---

### 2.A Core trait finalization

| Task | Detail | Effort |
|------|--------|--------|
| **2.A.1** Finalize `Identifier` trait | Define `fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate>`. Define `Candidate` struct (instrument, score, metadata map). | S |
| **2.A.2** Finalize `Timer` trait | Define `fn score(&self, candidate: &Candidate, ctx: &StrategyContext) -> TimingSignal`. Define `TimingSignal` (score 0.0–1.0, direction, confidence, rationale). | S |
| **2.A.3** Finalize `Sizer` trait | Define `fn size(&self, signal: &TimingSignal, ctx: &StrategyContext) -> PositionSize`. Define `PositionSize` (shares/units, notional value, risk fraction). | S |
| **2.A.4** Build `StrategyContext` | Struct that carries: instrument universe, bar snapshots (keyed by symbol + interval), latest fundamentals per instrument, latest macro series values, current portfolio state, current regime label, and the strategy parameter map. Load from `DataStore`. | M |
| **2.A.5** Define `StrategyConfig` and parameter versioning | Config struct with strategy ID, plugin name, enabled flag, parameter map (key-value), and version number. Backed by `strategy.configs` table. | S |

---

### 2.B Pipeline composition engine

| Task | Detail | Effort |
|------|--------|--------|
| **2.B.1** Implement `PipelineRunner` | Takes a `Vec<Box<dyn Identifier>>`, `Vec<Box<dyn Timer>>`, and `Vec<Box<dyn Sizer>>`. Runs identification → timing → sizing in sequence. Identifier outputs feed as Timer inputs; Timer scores averaged when multiple Timers present. | M |
| **2.B.2** Implement strategy run orchestration | `run_strategy(config, datastore) -> StrategyRunResult`. Loads context from datastore, runs pipeline, collects signals, persists them to `strategy.signals` and `strategy.runs`. | M |
| **2.B.3** Implement run result types | `StrategyRunResult` with run ID, timestamp, config snapshot, list of `TradeSignal` (candidate + timing score + position size + rationale). | S |

---

### 2.C First strategy plugins

| Task | Detail | Effort |
|------|--------|--------|
| **2.C.1** `strategy-momentum` Identifier | Rank instruments by risk-adjusted momentum: compute rolling return over configurable lookback (default 90 days), divide by rolling volatility. Return top-N candidates by score. Parameters: `lookback_days`, `top_n`. | M |
| **2.C.2** `strategy-mean-reversion` Timer | For each candidate: compute Bollinger Band position (how many std devs from 20-day SMA), RSI(14), and Z-score of price relative to rolling mean. Combine into a single 0–1 score. Configurable: `bb_period`, `rsi_period`, `zscore_window`, `signal_threshold`. | M |
| **2.C.3** `strategy-atr-sizer` Sizer | Compute position size as `(portfolio_value * risk_per_trade) / ATR(14)`. Cap at configurable max position fraction. Parameters: `risk_per_trade` (default 0.01 = 1%), `max_position_pct` (default 0.05 = 5%). | M |

---

### 2.D CLI wiring

| Task | Detail | Effort |
|------|--------|--------|
| **2.D.1** Add `run` subcommand to CLI | `economind run --strategy <config_id>` triggers a strategy run and prints the signals to stdout. | S |
| **2.D.2** Add `signals` subcommand to CLI | `economind signals [--since <date>] [--limit N]` queries and prints recent signals from the database. | S |

---

## 5. Phase 3 — Data Coverage

**Goal:** The platform feeds itself automatically. All free-tier data sources are implemented and scheduled.

**Exit criteria:**
- Daily OHLCV bars ingest automatically for all tracked instruments
- Fundamental data refreshes weekly from EDGAR/SimFin
- Macro series refresh daily from FRED
- `DataFeedManager` orchestrates all scheduled ingestion jobs

---

### 3.A Yahoo Finance connector

| Task | Detail | Effort |
|------|--------|--------|
| **3.A.1** Implement `YahooFinanceConnector` | Implement `DailyDataProvider` using the Yahoo Finance API (v8 chart endpoint, no auth required). Fetch OHLCV bars for a symbol over a date range. Handle rate limiting (politeness delay). | M |
| **3.A.2** Implement bulk download via Yahoo Finance | Add `fetch_all_instruments(symbols: &[Symbol], since: Date)` that batches requests with concurrency limiting. Write bars to `market.bars` via DataStore. | M |
| **3.A.3** Implement instrument metadata from Yahoo Finance | Fetch sector, market cap, description from Yahoo Finance quote endpoint. Upsert into `market.tickers`. | S |

---

### 3.B FRED macro connector

| Task | Detail | Effort |
|------|--------|--------|
| **3.B.1** Implement `FredConnector` | FRED API is free with registration (API key). Implement fetching for key series: `DGS10` (10Y treasury), `T10Y2Y` (yield curve), `CPIAUCSL` (CPI), `UNRATE` (unemployment), `VIXCLS` (VIX), `M2SL` (M2 money supply). Write to `market.macro_series`. | M |
| **3.B.2** Parameterize FRED series list | Store the list of tracked FRED series IDs in `strategy.configs` so they can be added/removed from the dashboard without code changes. | S |

---

### 3.C SEC EDGAR fundamentals connector

| Task | Detail | Effort |
|------|--------|--------|
| **3.C.1** Implement `EdgarConnector` | Use SEC EDGAR XBRL API (free, no auth). Fetch company facts (income statement, balance sheet, cash flow) by CIK number. Map to `IncomeStatement`, `BalanceSheet`, `CashFlowStatement` types. | L |
| **3.C.2** Build CIK lookup table | EDGAR identifies companies by CIK, not ticker. Build and cache a `ticker → CIK` mapping using SEC's company tickers JSON endpoint. | S |

---

### 3.D SimFin fundamentals connector

| Task | Detail | Effort |
|------|--------|--------|
| **3.D.1** Implement `SimFinConnector` | SimFin free API provides standardized annual and quarterly financials. Implement `FundamentalsProvider` using their REST API. Write to `market.fundamentals`. | M |

---

### 3.E Ingestion orchestration

| Task | Detail | Effort |
|------|--------|--------|
| **3.E.1** Implement `DataFeedManager` | The currently empty struct. Holds references to all active connectors and a scheduler. Exposes `run_scheduled_jobs()` and `run_job_by_name(name)`. | M |
| **3.E.2** Implement scheduler integration | Wire `tokio-cron-scheduler` into `DataFeedManager`. Store job schedules in `system.scheduled_jobs` table so they are configurable without restarting. | M |
| **3.E.3** Add `ingest` subcommand to CLI | `economind ingest bars --since <date>`, `economind ingest fundamentals`, `economind ingest macro`. Triggers specific ingestion jobs on demand. | S |

---

## 6. Phase 4 — Backtest Engine

**Goal:** A working backtest engine that can simulate a strategy run against historical data and produce performance metrics.

**Exit criteria:**
- `economind-backtest` runs a full strategy pipeline over a configurable historical date range
- Produces: equity curve, Sharpe ratio, max drawdown, win rate, avg win/loss, expectancy
- Results persisted to `backtest.runs` and `backtest.trades`
- CLI can trigger a backtest run and print a summary

---

### 4.A Backtest crate and simulation loop

| Task | Detail | Effort |
|------|--------|--------|
| **4.A.1** Create `economind-backtest` crate | New crate. Depends on `economind-core`, `economind-db`, `economind-strategy`. | S |
| **4.A.2** Implement `BacktestRunner` | Steps through historical dates one bar at a time. At each step, builds a `StrategyContext` from historical data (no lookahead), runs the pipeline, records simulated entries/exits. | L |
| **4.A.3** Implement simulated order execution | Given a signal, simulate order fill at next-day open (or configurable fill price: open, close, VWAP estimate). Account for slippage (configurable bps) and commission (configurable per-trade). | M |
| **4.A.4** Implement portfolio state tracking | Track simulated cash, open positions, and closed trades over the backtest period. Compute unrealized/realized P&L at each step. | M |

---

### 4.B Performance metrics

| Task | Detail | Effort |
|------|--------|--------|
| **4.B.1** Implement equity curve computation | From the trade log, produce a daily equity curve (portfolio value over time). | S |
| **4.B.2** Implement core metrics | Sharpe ratio (annualized, risk-free from FRED), Sortino ratio, max drawdown (peak-to-trough), max drawdown duration, CAGR. | M |
| **4.B.3** Implement trade-level metrics | Win rate, avg win, avg loss, profit factor, expectancy, avg hold duration, largest win, largest loss. | S |
| **4.B.4** Persist results | Write `BacktestRun` (metadata + all metrics) to `backtest.runs`. Write all simulated trades to `backtest.trades`. | S |

---

### 4.C CLI wiring

| Task | Detail | Effort |
|------|--------|--------|
| **4.C.1** Add `backtest` subcommand to CLI | `economind backtest run --strategy <config_id> --from <date> --to <date>`. Runs backtest and prints summary table. | S |

---

## 7. Phase 5 — API Layer

**Goal:** A complete REST + GraphQL + WebSocket API layer that exposes all platform operations.

**Exit criteria:**
- All endpoints listed in ARCHITECTURE.md §6 are implemented
- WebSocket signal streaming works
- Authentication (local API key) is enforced
- API serves the web dashboard static assets (placeholder at this stage)

---

### 5.A Axum server scaffold

| Task | Detail | Effort |
|------|--------|--------|
| **5.A.1** Set up Axum with shared state | Replace hello-world stub. Set up `AppState` containing `DataStore`, `StrategyEngine`, and `BacktestRunner` behind `Arc`. Add middleware: request logging, error handling, CORS (localhost only). | M |
| **5.A.2** Implement API key authentication | Middleware that checks `Authorization: Bearer <key>` header against a key stored in config/env. Returns 401 if missing or invalid. | S |

---

### 5.B REST endpoints

| Task | Detail | Effort |
|------|--------|--------|
| **5.B.1** Instrument endpoints | `GET /api/v1/instruments` (list/search), `GET /api/v1/instruments/:symbol` (detail with latest bars + fundamentals), `POST /api/v1/instruments` (add to universe), `DELETE /api/v1/instruments/:symbol`. | M |
| **5.B.2** Signal endpoints | `GET /api/v1/signals` (paginated, filterable by strategy / symbol / date / score threshold), `GET /api/v1/signals/:id`. | M |
| **5.B.3** Position endpoints | `GET /api/v1/positions` (current open), `GET /api/v1/positions/history` (closed), `GET /api/v1/positions/:id`. | M |
| **5.B.4** Strategy endpoints | `GET /api/v1/strategy/configs`, `GET /api/v1/strategy/configs/:id`, `PUT /api/v1/strategy/configs/:id` (update params), `POST /api/v1/strategy/run` (trigger on-demand run). | M |
| **5.B.5** Backtest endpoints | `POST /api/v1/backtest/run` (async, returns job ID), `GET /api/v1/backtest/:id` (results), `GET /api/v1/backtest` (list runs). | M |
| **5.B.6** Data endpoints | `GET /api/v1/data/bars?symbol=&from=&to=&interval=`, `GET /api/v1/data/fundamentals?symbol=`, `GET /api/v1/data/macro?series=`. | M |

---

### 5.C GraphQL

| Task | Detail | Effort |
|------|--------|--------|
| **5.C.1** Set up async-graphql | Add `async-graphql` and `async-graphql-axum`. Define schema root. Mount at `/graphql`. Add GraphiQL playground at `/graphiql` (dev mode only). | M |
| **5.C.2** Implement core query types | `instrument`, `signals`, `positions`, `backtestRun`, `strategyConfigs` — matching the planned schema. | L |
| **5.C.3** Implement mutations | `triggerStrategyRun`, `updateStrategyConfig`, `addInstrument`, `removeInstrument`. | M |

---

### 5.D WebSocket

| Task | Detail | Effort |
|------|--------|--------|
| **5.D.1** Implement signal streaming | `WS /ws/signals`. On connection (with valid API key), subscribe to an internal broadcast channel. When a strategy run emits signals, broadcast typed JSON events to all connected WS clients. | M |
| **5.D.2** Implement event types | `SignalEmitted`, `StrategyRunStarted`, `StrategyRunCompleted`, `IngestionJobCompleted`, `PositionOpened`, `PositionClosed`, `SystemError`. | S |

---

## 8. Phase 6 — Web Dashboard

**Goal:** A SvelteKit single-page application that provides full visibility and control of the platform via the API.

**Exit criteria:**
- All seven views from ARCHITECTURE.md §7.1 are implemented
- Dashboard is bundled and embedded in the `economind-api` binary
- Live signal feed via WebSocket works

---

### 6.A SvelteKit project setup

| Task | Detail | Effort |
|------|--------|--------|
| **6.A.1** Scaffold SvelteKit project | Create `dashboard/` at workspace root. Configure for static adapter (no SSR). Set API base URL from environment. Set up TypeScript, Tailwind CSS. | M |
| **6.A.2** Set up API client layer | Type-safe API client generated from OpenAPI spec or hand-written to match REST endpoints. WebSocket client with reconnect logic. | M |
| **6.A.3** Embed in binary | Cargo build script that runs `npm run build` and embeds the `build/` output using `include_dir!`. Axum serves at `/`. | M |

---

### 6.B Dashboard views

| Task | Detail | Effort |
|------|--------|--------|
| **6.B.1** Overview / home view | Live signal feed (WebSocket), open positions summary, today's P&L, recent strategy run status, key system metrics. | M |
| **6.B.2** Signals Explorer | Table with search, filter by strategy / symbol / score / date range. Click signal for full detail (context snapshot, rationale, linked backtest performance). | M |
| **6.B.3** Portfolio view | Open positions table with live P&L (mark-to-market from latest bar). Closed trades history. Simple equity chart. | M |
| **6.B.4** Strategy Manager | List of strategy configs. Edit parameters inline. Enable/disable toggle. Manual run trigger. Run history with outcome summary. | M |
| **6.B.5** Backtest view | Form to configure and launch a backtest (strategy, date range, initial capital). Results panel: equity curve chart, metrics table, trade log. | L |
| **6.B.6** Data Explorer | Instrument browser with bar chart, latest fundamentals, news feed. DuckDB ad hoc query editor (POST query → tabular results). | L |
| **6.B.7** Settings view | API key management for data sources and brokers. Ingestion schedule display and manual trigger. System health indicators. | M |

---

## 9. Phase 7 — Agentic Layer

**Goal:** The platform exposes an MCP server endpoint so Claude (or any MCP client) can query and reason about platform data. A built-in analysis assistant can digest trade candidates and produce natural-language summaries.

**Exit criteria:**
- MCP server serves tool endpoints queryable from Claude
- At least five MCP tools implemented (signal query, instrument lookup, portfolio state, backtest summary, trigger run)
- Analysis assistant can produce a natural-language brief on a given instrument/signal

---

### 7.A MCP server

| Task | Detail | Effort |
|------|--------|--------|
| **7.A.1** Set up `rmcp` in `economind-agentic` | Add the `rmcp` crate (Rust MCP SDK). Implement `McpServer` struct backed by `DataStore`. Expose on a configurable port (default: 8081, separate from the REST API port 8080). | M |
| **7.A.2** Implement `get_signals` tool | MCP tool that queries recent signals with optional filters (symbol, strategy, since date, min score). Returns structured JSON. | S |
| **7.A.3** Implement `get_instrument` tool | Given a symbol, return current price, latest fundamentals, recent bars summary, and any active signals. | S |
| **7.A.4** Implement `get_portfolio` tool | Return current open positions, total portfolio value, unrealized P&L, and cash available. | S |
| **7.A.5** Implement `get_backtest_summary` tool | Given a backtest run ID (or "latest"), return key performance metrics in structured form. | S |
| **7.A.6** Implement `trigger_strategy_run` tool | Trigger an on-demand strategy run (same as `POST /api/v1/strategy/run`). Return run ID for polling. | S |
| **7.A.7** Implement `query_bars` tool | Return OHLCV bar data for a symbol over a date range. Supports configurable interval. | S |
| **7.A.8** Implement `get_macro_context` tool | Return current values of all tracked FRED macro series with a short label for each. Useful for LLM context injection. | S |

---

### 7.B Opportunity analysis assistant

| Task | Detail | Effort |
|------|--------|--------|
| **7.B.1** Design analysis prompt templates | Create structured prompt templates for: (a) instrument brief (price action + fundamentals + news summary), (b) signal rationale review (does the signal make intuitive sense given the data?), (c) macro context summary (what is the current macro environment saying?). | M |
| **7.B.2** Implement `LlmClient` in `economind-agentic` | Trait-based LLM client with two concrete backends: `AnthropicBackend` (wraps Anthropic Messages API, model default: `claude-haiku-4-5`) and `LocalBackend` (wraps any OpenAI-compatible `/v1/chat/completions` endpoint, e.g. Ollama or llama.cpp server, model default: `llama3`). A `LlmClientConfig::from_env()` constructor reads `ANTHROPIC_API_KEY`, `LOCAL_LLM_BASE_URL`, `LOCAL_LLM_MODEL`, and `LLM_PROVIDER` and returns the appropriate backend, or `None` if neither is configured. Callers check for `None` and skip analysis gracefully. | M |
| **7.B.3** Implement `analyze_signal` function | Given a `TradeSignal`, pulls instrument data, news, and macro context from DataStore, constructs a prompt, calls Anthropic API, and returns a natural-language brief. Persist brief alongside signal in `strategy.signals`. | M |
| **7.B.4** Add `analyze` subcommand to CLI | `economind analyze signal <signal_id>` — fetches signal, runs analysis, prints brief. `economind analyze instrument <symbol>` — produces current instrument brief. | S |
| **7.B.5** Expose analysis via MCP | Add `analyze_signal` and `analyze_instrument` MCP tools that trigger the analysis assistant and return the brief as a string. | S |

---

## 10. Phase 8 — Composition Modes & Optimization

**Goal:** Add Voting and Ensemble composition modes. Add ensemble weight optimization via backtest. Add the HMM regime classifier.

**Exit criteria:**
- All three composition modes work and are selectable per strategy config
- Ensemble weights can be optimized automatically by maximizing Sharpe ratio over a backtest period
- HMM regime classifier operational as an Identifier plugin
- Remaining strategy plugins implemented (trend-follow timer, Kelly sizer)

---

### 8.A Voting / Consensus mode

| Task | Detail | Effort |
|------|--------|--------|
| **8.A.1** Implement `VotingRunner` | Takes a `Vec<StrategyStack>` (each stack = one Identifier + one Timer + one Sizer). Runs all stacks in parallel. Tallies binary votes per instrument. Emits signals for instruments meeting quorum. | M |
| **8.A.2** Configurable quorum threshold | Quorum stored in strategy config. Validate that quorum ≤ number of stacks. | S |

---

### 8.B Ensemble / Weighted mode

| Task | Detail | Effort |
|------|--------|--------|
| **8.B.1** Implement `EnsembleRunner` | Takes `Vec<(StrategyStack, f32)>` (stack + weight). Runs all stacks, computes weighted sum of signal scores. Instruments above threshold proceed. | M |
| **8.B.2** Implement weight optimization | Given a date range and target metric (default: Sharpe ratio), run a grid search or Nelder-Mead simplex over the weight vector. Store optimal weights back to strategy config. | L |

---

### 8.C Additional strategy plugins

| Task | Detail | Effort |
|------|--------|--------|
| **8.C.1** `strategy-trend-follow` Timer | EMA crossover (fast/slow configurable) + ADX threshold confirmation on daily and weekly timeframes. Configurable: `fast_ema`, `slow_ema`, `adx_period`, `adx_threshold`. | M |
| **8.C.2** `strategy-kelly-sizer` Sizer | Query historical win rate and avg win/loss from `backtest.trades` for the given strategy config. Apply fractional Kelly with configurable fraction (default 0.25). Fallback to ATR sizer if insufficient backtest history. | M |
| **8.C.3** `strategy-regime` Identifier | HMM-based regime classifier using `linfa-hmm` or a hand-rolled Baum-Welch implementation. Features: rolling volatility, rolling return, volume ratio. States: trending-up, trending-down, ranging, high-volatility. Pass candidates only when regime is favorable. | L |

---

## 11. Effort Key

| Label | Meaning | Typical scope |
|-------|---------|--------------|
| **S** — Small | Single-focus task, well-understood implementation | < 2 hours |
| **M** — Medium | Moderate complexity, some design decisions | 2–6 hours |
| **L** — Large | High complexity, multiple sub-problems, or significant research | 6–16 hours |

---

## 12. Resolved Decisions

All architectural decisions are resolved. These are binding for implementation.

| # | Decision | Resolution |
|---|----------|-----------|
| **D1** | Where do `Identifier`, `Timer`, `Sizer` traits live? | **`economind-strategy`**. `economind-core` holds only data types and the `DataStore` trait. Strategy-concept traits belong in the strategy crate to keep `core` dependency-free. |
| **D2** | Which MCP Rust SDK to use? | **`rmcp`** (official Anthropic Rust SDK). Saves significant boilerplate vs. a hand-rolled JSON-RPC implementation. |
| **D3** | LLM provider configuration for the agentic layer. | **Dual-provider, fully optional.** The analysis assistant supports two backends: (1) **Anthropic API** (cloud) via `ANTHROPIC_API_KEY`, and (2) **local inference server** (any OpenAI-compatible endpoint, e.g. Ollama, llama.cpp) via `LOCAL_LLM_BASE_URL` + `LOCAL_LLM_MODEL`. Selection: cloud if key present, local if URL configured, cloud preferred when both set (overridable via `LLM_PROVIDER=local\|cloud`). If neither configured, all analysis features disable gracefully — the rest of the platform is unaffected. Model defaults: cloud = `claude-haiku-4-5`, local = `llama3`. |
| **D4** | Broker integration timing. | **Phase 3.F** — Alpaca connector added after free-data connectors are complete. IBKR deferred to a later phase. |
| **D5** | DuckDB sync strategy. | **Full snapshot** (PostgreSQL → DuckDB parquet export) on each strategy run. Move to incremental delta sync only if sync time becomes a measurable bottleneck. |
| **D6** | Dashboard embedding strategy. | **Filesystem in dev, `include_dir!` in release.** Controlled by `ECONOMIND_DASHBOARD_PATH` env var — if set, serves from that path (dev); if unset, serves embedded assets (release). |

---

*Economind Implementation Plan — Internal — May 2026*
