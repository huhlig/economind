# Economind Platform Architecture

**Low-Frequency Algorithmic Trading Platform**
Rust · PostgreSQL · DuckDB · Pluggable Strategy Engine
*Version 1.0 — May 2026*

---

## Table of Contents

1. [Overview](#1-overview)
2. [High-Level Architecture](#2-high-level-architecture)
3. [Workspace Crate Structure](#3-workspace-crate-structure)
4. [Strategy Engine](#4-strategy-engine)
5. [Data Layer](#5-data-layer)
6. [API Layer](#6-api-layer)
7. [Web Dashboard](#7-web-dashboard)
8. [Scheduling and Lifecycle](#8-scheduling-and-lifecycle)
9. [Deployment](#9-deployment)
10. [Technology Stack Summary](#10-technology-stack-summary)
11. [Open Decisions](#11-open-decisions)
12. [Implementation Roadmap](#12-implementation-roadmap)

---

## 1. Overview

Economind is a single-user, low-frequency algorithmic trading platform targeting trade durations of one day to one month. It is designed to identify optimal trade opportunities across equities, ETFs, and similar instruments, then determine precise entry timing and position sizing — all through a unified, pluggable strategy engine.

The platform is implemented entirely in Rust for performance, memory safety, and long-term maintainability. It uses PostgreSQL as the primary operational database for time-series market data, positions, and audit history, and DuckDB as an embedded analytical engine for in-process backtesting and strategy signal computation. A REST + GraphQL API layer exposes all platform capabilities to an integrated web dashboard and any external tooling.

### 1.1 Design Goals

| Goal | Description |
|------|-------------|
| **Pluggable strategies** | Instrument identification, timing, and sizing are each independently pluggable. Strategies can be combined in any of three modes: pipeline, voting consensus, or ensemble weighting. |
| **Data-first** | All market, fundamental, and alternative data is ingested, normalized, and stored before any strategy runs. Strategies operate on clean, structured data — never raw feeds. |
| **Free-data preference** | The platform is designed to operate on freely available data (Yahoo Finance, EDGAR, FRED, etc.). Paid data can be added as a drop-in connector. |
| **Auditability** | Every signal, trade decision, and execution is logged with full context so every decision can be replayed and explained. |
| **Local / self-hosted** | No cloud dependency. Runs on a laptop or a self-hosted Linux server. A single binary plus a config file is sufficient to start. |
| **Strategy composition** | Strategies interact via three composable modes that can be chosen per run: sequential pipeline, multi-strategy voting, and weighted ensemble. |

---

## 2. High-Level Architecture

The platform is organized as a Cargo workspace of focused crates. Each crate has a single responsibility and communicates with others through well-defined Rust traits and async message channels.

### 2.1 Layer Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                     Web Dashboard (SvelteKit)                   │
│                     served by platform binary                   │
└────────────────────────────┬────────────────────────────────────┘
                             │ HTTP / WebSocket
┌────────────────────────────▼────────────────────────────────────┐
│               REST + GraphQL API  (Axum)                        │
│               economind-api                                     │
└────────────────────────────┬────────────────────────────────────┘
                             │ Rust traits / async channels
┌────────────────────────────▼────────────────────────────────────┐
│           Platform Orchestrator  (scheduler, event bus)         │
│           economind-strategy                                    │
└──────────┬─────────────────┬──────────────────┬────────────────┘
           │                 │                  │
     ┌─────▼──────┐   ┌──────▼──────┐   ┌──────▼──────┐
     │ Identifier │   │   Timer     │   │   Sizer     │
     │ plugins    │   │   plugins   │   │   plugins   │
     └─────┬──────┘   └──────┬──────┘   └──────┬──────┘
           └─────────────────┼──────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│           Signal & Indicator Library                            │
│           economind-indicators                                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│           Data Access Layer  (DataStore trait)                  │
│           economind-db                                          │
└──────────────────┬──────────────────────┬───────────────────────┘
                   │                      │
         ┌─────────▼──────┐      ┌────────▼──────┐
         │  PostgreSQL     │      │   DuckDB      │
         │  (operational)  │      │   (analytics) │
         └────────┬────────┘      └───────────────┘
                  │
┌─────────────────▼───────────────────────────────────────────────┐
│           Data Ingestion  (connector trait)                     │
│           economind-ingest                                      │
└─────────────────────────────────────────────────────────────────┘
          │           │           │           │
     Yahoo Finance  EDGAR       FRED       Broker API
```

### 2.2 Data Flow

**Inbound (collection):** External data sources → Ingestion connectors → normalization → PostgreSQL time-series tables → materialized views and DuckDB snapshots for fast query.

**Outbound (decision):** Scheduler triggers strategy run → Identification stage filters the instrument universe → Timing stage scores each candidate → Sizing stage computes position size → Signal is emitted to the API / dashboard and optionally forwarded to the broker execution connector.

---

## 3. Workspace Crate Structure

The Cargo workspace is organized as follows. Each crate is independently testable.

### 3.1 Core Crates

| Crate | Role | Responsibility |
|-------|------|----------------|
| `economind-core` | Shared types, traits, errors, configuration | Defines the canonical data model: `Instrument`, `Bar`, `Signal`, `TradeDecision`, `Position`, `StrategyConfig`. Zero runtime dependencies. All other crates depend on this one. |
| `economind-db` | Database access layer | Owns all SQL migrations, connection pooling (sqlx), and the DuckDB embedded engine. Exposes a unified `DataStore` trait so upper layers are database-agnostic. |
| `economind-ingest` | Data ingestion | Implements a `DataConnector` trait with adapters for Yahoo Finance, Alpha Vantage, FRED, EDGAR/SEC, SimFin, and broker APIs. Runs scheduled polling jobs. |
| `economind-indicators` | Signal & indicator library | Pure-function indicator implementations (SMA, EMA, RSI, MACD, Bollinger Bands, ATR, P/E normalizers, yield curve features, etc.). Stateless and independently testable. |
| `economind-strategy` | Strategy engine and plugin host | Owns the three strategy trait definitions (`Identifier`, `Timer`, `Sizer`) and the three composition engines (Pipeline, Voter, Ensemble). Loads strategy plugins and orchestrates runs. |
| `economind-backtest` | Historical simulation | Runs strategies against historical DuckDB data. Computes Sharpe ratio, max drawdown, win rate, expectancy. Results persisted to PostgreSQL. |
| `economind-api` | REST + GraphQL API server | Axum-based HTTP server. Handles auth, WebSocket streaming for live signals, serves web dashboard static assets. |
| `economind-cli` | Command-line interface | clap-based CLI for running ingestion, triggering strategy runs, viewing signals, and managing configuration. |

### 3.2 Strategy Plugin Crates

Each strategy is a separate crate implementing one or more core traits. Plugins are compiled statically into the binary for production or loaded as `cdylib` dynamic libraries for development hot-reloading.

| Crate | Trait | Description |
|-------|-------|-------------|
| `strategy-regime` | `Identifier` | Hidden Markov Model market regime classifier. Classifies the current market into regimes (trending, ranging, high-volatility) using historical volatility and return features. |
| `strategy-momentum` | `Identifier` | Cross-sectional momentum screener. Ranks instruments by risk-adjusted momentum over configurable lookback windows and emits the top-N candidates. |
| `strategy-mean-reversion` | `Timer` | Uses Bollinger Bands, Z-score, and RSI to identify over-extended instruments. Outputs entry/exit signal scores. |
| `strategy-trend-follow` | `Timer` | Multi-timeframe trend confirmation using EMA crossovers and ADX across daily and weekly timeframes. Reduces false entries. |
| `strategy-atr-sizer` | `Sizer` | ATR-based volatility position sizer. Computes size as a fixed fraction of portfolio risk divided by ATR. Scales down in high-volatility regimes. |
| `strategy-kelly-sizer` | `Sizer` | Fractional Kelly criterion sizer. Uses historical win rate and avg win/loss ratio from backtest results with a configurable scaling factor. |

---

## 4. Strategy Engine

The strategy engine turns a universe of instruments and market data into a ranked list of trade recommendations with sizes. It supports three composition modes selectable per strategy run via configuration.

### 4.1 Core Trait Definitions

All strategies implement one of three Rust traits defined in `economind-core`:

```rust
pub trait Identifier: Send + Sync {
    fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate>;
}

pub trait Timer: Send + Sync {
    fn score(&self, candidate: &Candidate, ctx: &StrategyContext) -> TimingSignal;
}

pub trait Sizer: Send + Sync {
    fn size(&self, signal: &TimingSignal, ctx: &StrategyContext) -> PositionSize;
}
```

Each trait is parameterized by a `StrategyContext` that carries the current portfolio state, available market data, configuration parameters, and the current regime classification. Strategies are stateless with respect to the engine — they read from context and return values, with no side effects.

### 4.2 Composition Mode 1 — Pipeline

Stages run sequentially. The output of each stage is the input to the next. This is the default and recommended mode.

```
Full instrument universe
         │
         ▼
  [ Identifier(s) ] ──► filtered candidate list (e.g., top 20)
         │
         ▼
  [ Timer(s)      ] ──► scored signals (0.0–1.0 entry attractiveness)
         │
         ▼
  [ Sizer(s)      ] ──► final trade decisions with position sizes
```

When multiple strategies are configured at a single stage in pipeline mode, they run in series (for Identifiers, each further filters the list) or their scores are averaged (for Timers) before passing to the next stage.

### 4.3 Composition Mode 2 — Voting / Consensus

Multiple complete strategy stacks run in parallel on the same instrument universe. Each stack produces a binary recommendation (buy / no-buy) per instrument. A trade recommendation is emitted only when a configurable quorum threshold is reached (e.g., 3 of 5 strategies must agree).

- Reduces false positives significantly — all strategies must agree before a trade fires.
- Works best when strategies are meaningfully diverse (momentum + mean reversion + macro).
- Quorum threshold is a tunable parameter stored in the strategy configuration table.

### 4.4 Composition Mode 3 — Ensemble / Weighted

Multiple complete strategy stacks each produce a continuous signal strength (0.0–1.0) per instrument. The engine applies a learned or manually configured weight vector and sums to a composite score. Instruments above a configurable threshold proceed to sizing.

- Weights can be set manually or optimized via the backtest module using Sharpe ratio maximization.
- Allows soft combination of conflicting signals rather than binary voting.
- Ensemble weights are versioned and stored in PostgreSQL for historical comparison.

### 4.5 Strategy Context

Every strategy receives a `StrategyContext` at runtime containing:

- Market data snapshots (OHLCV bars up to configurable lookback)
- Fundamental data (latest P/E, revenue growth, debt ratios per instrument)
- Macro indicators (FRED series: yield curve, CPI, unemployment, VIX)
- Current portfolio state (open positions, available capital, drawdown)
- Regime classification output from the regime classifier
- Strategy-specific parameter map (key-value, stored in DB, editable via dashboard)

Parameters are stored in PostgreSQL and versioned. Changes take effect on the next scheduled run, with the previous parameter set retained for audit and backtest replay.

---

## 5. Data Layer

The data layer is split between two database engines with complementary strengths, managed transparently by the `DataStore` abstraction in `economind-db`.

### 5.1 PostgreSQL — Operational Store

PostgreSQL serves as the primary operational database. TimescaleDB is the recommended extension for efficient time-series storage, automatic partitioning, compression, and time-bucket aggregation.

| Schema / Table | Contents |
|---------------|----------|
| `market.bars` | OHLCV bars for all tracked instruments — partitioned by instrument and interval (1d, 1h, etc.) |
| `market.fundamentals` | Earnings, revenue, balance sheet, P/E, keyed by instrument and report date |
| `market.macro_series` | FRED macro time series (CPI, yield spreads, VIX, etc.) |
| `strategy.configs` | Strategy plugin configurations and versioned parameter sets |
| `strategy.signals` | Full history of every signal emitted by every strategy run |
| `portfolio.positions` | Current and historical positions with entry/exit metadata |
| `portfolio.trades` | Executed trade log with broker confirmation data |
| `backtest.runs` | Backtest run metadata, parameters, and aggregated performance metrics |
| `backtest.trades` | Simulated trades from each backtest run |
| `system.audit_log` | Immutable append-only log of all state-changing events |

### 5.2 DuckDB — Analytical Engine

DuckDB runs embedded within the Rust process via `duckdb-rs`. It is not a separate service — it opens a file on disk and exposes a high-performance columnar query engine to the strategy and backtest layers.

- **Strategy signal computation:** Indicator calculations over large bar windows run as columnar SQL on DuckDB snapshots rather than row-by-row Rust code. This is 10–100x faster for window functions.
- **Backtesting:** The entire backtest simulation runs in DuckDB against historical data snapshots. Results are written back to PostgreSQL.
- **Ad hoc analysis:** The CLI and dashboard provide a DuckDB query endpoint for exploratory analysis.

DuckDB data is refreshed from PostgreSQL at each strategy run (or on a configurable schedule) via a parquet export or PostgreSQL foreign data wrapper.

### 5.3 Data Ingestion Connectors

| Connector | Source / Cost | Data Type |
|-----------|--------------|-----------|
| `yahoo-finance` | Yahoo Finance — Free | OHLCV bars (daily, weekly), basic fundamentals |
| `alpha-vantage` | Alpha Vantage — Free tier (25 calls/day); paid tiers available | Intraday bars, extended fundamentals, earnings calendar |
| `fred` | Federal Reserve FRED — Free | Macro series: CPI, yield curve, unemployment, M2 |
| `sec-edgar` | SEC EDGAR — Free | 10-K/10-Q filings, income statement, balance sheet |
| `simfin` | SimFin — Free tier available | Standardized fundamental data across US equities |
| `alpaca-broker` | Alpaca — Free paper / live account required for trading | Broker execution, live quotes, account state |
| `ibkr-broker` | Interactive Brokers — Account required | Broker execution, live data, options data |

---

## 6. API Layer

The API layer is implemented in `economind-api` using Axum. It serves both a REST API and a GraphQL endpoint from the same binary, and also serves the web dashboard as static assets. Authentication is via a local API key (suitable for single-user, local/self-hosted deployment).

### 6.1 REST Endpoints

| Endpoint Group | Key Operations |
|---------------|----------------|
| `GET  /api/v1/instruments` | List tracked instruments, search by symbol or sector, add/remove from universe |
| `GET  /api/v1/signals` | Paginated signal history, filter by strategy / instrument / date range |
| `GET  /api/v1/positions` | Current open positions with P&L; historical closed positions |
| `POST /api/v1/strategy/run` | Trigger an on-demand strategy run with optional parameter overrides |
| `GET  /api/v1/strategy/configs` | List and manage strategy configurations; update parameters |
| `POST /api/v1/backtest/run` | Launch a backtest run; returns job ID for async polling |
| `GET  /api/v1/backtest/{id}` | Retrieve backtest results, trade log, and performance metrics |
| `GET  /api/v1/data/bars` | Query OHLCV bars for one or more instruments over a date range |
| `WS   /ws/signals` | Real-time stream of signals as they are emitted by strategy runs |

### 6.2 GraphQL Schema (Key Types)

The GraphQL endpoint (via `async-graphql`) exposes the same data model as REST but allows flexible, nested queries. Key root query types:

- `instrument(symbol: String!): Instrument` — full detail including latest bars and fundamentals
- `signals(filter: SignalFilter): [Signal]` — flexible signal queries with strategy/date/score filters
- `positions: [Position]` — current portfolio state
- `backtestRun(id: ID!): BacktestRun` — full backtest result including simulated trade log
- `strategyConfigs: [StrategyConfig]` — all configured strategy stacks and their parameters

### 6.3 WebSocket Streaming

The `/ws/signals` channel delivers real-time typed JSON events to the dashboard: signal emitted, strategy run started/completed, ingestion job completed, position opened/closed, and system error. The dashboard uses these events to update live without polling.

---

## 7. Web Dashboard

The web dashboard is a single-page application bundled as static assets and served by `economind-api`. It communicates exclusively with the REST and GraphQL API. The recommended frontend framework is SvelteKit compiled to static files and embedded in the binary via Rust's `include_dir` macro.

### 7.1 Dashboard Views

| View | Contents |
|------|----------|
| **Overview** | Live signal feed, open positions summary, today's P&L, recent strategy runs, key metrics |
| **Signals Explorer** | Full searchable/filterable signal history with strategy attribution and entry/exit context |
| **Portfolio** | Open positions with live P&L, closed trade history, position-level analytics |
| **Strategy Manager** | List of configured strategy stacks; edit parameters; enable/disable; trigger manual runs |
| **Backtest** | Configure and launch backtests; view equity curve, performance metrics, and trade log |
| **Data Explorer** | Browse ingested market, fundamental, and macro data; run DuckDB ad hoc queries |
| **Settings** | API keys for data sources and brokers, schedule configuration, ingestion status |

---

## 8. Scheduling and Lifecycle

The platform orchestrator runs a built-in async scheduler (`tokio-cron-scheduler`). All scheduled jobs are stored in PostgreSQL and can be modified via the dashboard or CLI without restarting the binary.

| Job | Default Schedule | Description |
|-----|-----------------|-------------|
| Bar ingestion | Daily 5:00 PM ET | Fetch prior-day OHLCV bars for all tracked instruments |
| Fundamental refresh | Weekly Sunday 6:00 PM ET | Update fundamental data from EDGAR and SimFin |
| Macro data refresh | Daily 6:00 PM ET | Pull latest FRED macro series |
| Strategy run | Daily 6:30 PM ET | Run all enabled strategy stacks; emit signals |
| DuckDB refresh | Before each strategy run | Sync DuckDB snapshot from PostgreSQL |
| Broker sync | Every 5 min (market hours) | Sync open positions and fill notifications from broker API |

---

## 9. Deployment

The entire platform compiles to a single binary (`cargo build --release`). Configuration is provided via a TOML file and environment variables. The binary includes the embedded web dashboard and all static assets.

### 9.1 Local Deployment

```bash
# Install PostgreSQL + TimescaleDB extension
# Set config in economind.toml (database URL, API keys, etc.)
./economind serve
# Dashboard available at http://localhost:8080
```

### 9.2 Self-Hosted Server (Docker Compose)

A Docker Compose file will be provided with three services:

- `economind` — the platform binary
- `postgres` — PostgreSQL + TimescaleDB
- `caddy` — reverse proxy with automatic HTTPS for LAN access

### 9.3 Configuration

Configuration is layered: `economind.toml` (baseline) → environment variables (overrides) → database-stored strategy configs (runtime). Secrets (API keys, database passwords) are always loaded from environment variables, never stored in the TOML file.

---

## 10. Technology Stack Summary

| Concern | Technology | Crate / Tool |
|---------|-----------|--------------|
| Language | Rust (stable) | cargo workspace |
| Async runtime | Tokio | `tokio` |
| Web framework | Axum | `axum` |
| GraphQL | async-graphql | `async-graphql` |
| SQL (PostgreSQL) | sqlx (async, compile-time checked) | `sqlx` |
| Time-series | TimescaleDB extension | PostgreSQL extension |
| Analytics engine | DuckDB (embedded) | `duckdb-rs` |
| Serialization | Serde + serde_json | `serde` |
| Config | TOML + environment variables | `config`, `dotenvy` |
| CLI | clap | `clap` |
| Scheduler | tokio-cron-scheduler | `tokio-cron-scheduler` |
| ML / statistics | linfa (Rust-native) | `linfa` |
| Frontend framework | SvelteKit (or React + Vite) | npm / bun |
| Migrations | sqlx-cli | `sqlx-cli` |
| Logging | tracing + tracing-subscriber | `tracing` |
| Error handling | thiserror + anyhow | `thiserror`, `anyhow` |

---

## 11. Open Decisions

The following architectural decisions remain open and should be resolved before implementation begins:

| Decision | Options | Recommendation |
|----------|---------|----------------|
| **Strategy composition default** | Pipeline / Voting / Ensemble | Begin with Pipeline — simplest to reason about and debug. Add Voting and Ensemble once core is stable. |
| **Frontend framework** | SvelteKit vs React + Vite | SvelteKit — smaller bundle, excellent TypeScript support, easy to embed as static assets. |
| **Strategy plugin loading** | Static (compiled in) vs dynamic (cdylib) | Static for production; dynamic only for development hot-reload. |
| **Paid data sources** | Alpha Vantage premium, Polygon.io, Refinitiv | Defer until free-tier data sources are fully integrated and gaps are identified. |
| **Live trading automation** | Full auto-execution vs signal-only with manual confirmation | Signal-only first — review signals before execution. Auto-execution as opt-in later. |
| **ML / statistical backends** | Native Rust (linfa), Python subprocess, embedded WASM | Start with Rust-native (linfa for HMM/regression). Python subprocess for heavier models if needed. |

---

## 12. Implementation Roadmap

| Phase | Deliverables |
|-------|-------------|
| **Phase 1 — Foundation** | `economind-core` types and traits; `economind-db` with PostgreSQL schema and migrations; basic ingestion for Yahoo Finance; CLI scaffold |
| **Phase 2 — Strategy Engine** | `economind-strategy` with pipeline composition; first Identifier (momentum) and first Timer (mean-reversion); ATR sizer; end-to-end signal generation |
| **Phase 3 — Backtest** | `economind-backtest` with DuckDB integration; performance metrics; backtest results stored in PostgreSQL |
| **Phase 4 — API + Dashboard** | `economind-api` with REST + GraphQL; WebSocket streaming; SvelteKit dashboard with Overview, Signals, Portfolio, and Strategy Manager views |
| **Phase 5 — Composition Modes** | Add Voting and Ensemble composition modes; weight optimization via backtest; HMM regime classifier |
| **Phase 6 — Full Data Coverage** | EDGAR, FRED, SimFin connectors; macro indicators in StrategyContext; broker execution connectors (Alpaca first, IBKR second) |

---

*Economind — Internal Architecture Document — Confidential*
