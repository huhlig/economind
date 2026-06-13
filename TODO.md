# Economind 1.0 — Work Tracking

> Ordered by priority. P0 = blocks compilation/operation. P1 = blocks real use.
> P2 = makes it a trading platform. P3 = production quality.

---

## P0 — Make It Compile

- [x] **Fix `strategy-regime` borrow errors** (`strategies/strategy-regime/src/lib.rs:430`)
  - `alpha`, `beta`, and `self` are moved into `FnMut` closures inside a loop in the Baum-Welch E-step
  - Compiler suggests cloning `alpha`/`beta` before the inner closure — apply that fix
  - Also fix: `self.log_a` borrow after move (restructure to avoid moving `self` into closure)

- [x] **Fix `economind-agentic` rmcp v1.6 API mismatch** (`agentic/src/mcp.rs:519,527`)
  - `call_tool` no longer exists on `ToolRouter<S>` in rmcp v1.6
  - `list_tools` renamed to `list_all`
  - Check rmcp v1.6 changelog/docs for correct `call_tool` replacement and fix both methods

- [x] **Fix compiler warnings** (treat as errors before 1.0)
  - Remove unused import `economind_core::model::Symbol` in `strategy/src/stack.rs:18`
  - Prefix unused `n` in `strategy/src/ensemble.rs:422` with `_n`
  - Prefix unused `ctx` in `strategy/src/stack.rs:180` with `_ctx`
  - Prefix unused `avg_vol` in `strategies/strategy-regime/src/lib.rs:214` with `_avg_vol`

- [x] **Fix sqlx 0.9 / reqwest 0.13 feature name renames** (workspace Cargo.toml: `runtime-tokio-rustls` → `runtime-tokio` + `tls-rustls-ring`; `rustls-tls` → `rustls`; `AssertSqlSafe` wrapper for dynamic SQL in `postgres_strategy.rs`)

- [x] **SQLX offline mode — N/A** (project uses `sqlx::query()` runtime calls throughout, zero `query!` macros — no DB required at compile time)

---

## P0 — Operational Baseline

- [x] **Ticker universe bootstrap**
  - Add `universe.csv` at repo root with ~500 S&P 500 symbols (symbol, exchange, name, sector)
  - Add CLI command: `economind universe add <symbol>` and `economind universe list`
  - Add `economind universe load --file universe.csv` to bulk-load from CSV
  - On first `economind run`, check if universe is empty and warn clearly

- [x] **Scheduler wiring**
  - Add `tokio-cron-scheduler` to workspace `Cargo.toml`
  - Implement scheduler in `economind-api` (or a new `economind-scheduler` module in `strategy`)
  - Wire the five daily jobs from ARCHITECTURE.md §8:
    - Daily 5:00 PM ET — bar ingestion for all tracked symbols
    - Daily 6:00 PM ET — macro data refresh (FRED)
    - Weekly Sunday 6:00 PM ET — fundamental refresh (EDGAR + SimFin)
    - Before each strategy run — DuckDB sync from PostgreSQL
    - Daily 6:30 PM ET — strategy run (all enabled configs)
  - Store schedules in `system.scheduled_jobs` table; configurable without restart
  - Add `economind scheduler status` CLI subcommand to show next-run times

- [x] **`economind.toml` config file**
  - Define TOML schema covering: database URL, DuckDB path, API port, API key, LLM settings,
    schedule enable/disable flags, ingestion concurrency, slippage/commission defaults
  - Add `config` crate or hand-rolled TOML loader; layer under env var overrides
  - Add `economind.toml.example` to repo root
  - Document: secrets always via env var, non-secret settings in TOML

- [x] **Docker Compose deployment**
  - `docker-compose.yml` with three services:
    - `postgres` — TimescaleDB image (`timescale/timescaledb:latest-pg16`)
    - `economind` — platform binary
    - `caddy` — reverse proxy with automatic HTTPS for LAN access
  - `docker/init.sql` that runs migrations on first postgres start
  - `.env.example` with all required environment variables
  - `Dockerfile` for the platform binary (multi-stage: build + minimal runtime image)
  - Verify `cargo build --release` produces a single static binary

- [x] **Setup documentation** (`SETUP.md`)
  - Prerequisites: Rust stable, PostgreSQL (or Docker), Node.js for dashboard dev
  - Step-by-step: clone → configure → database setup → first universe load → first run
  - FRED API key registration instructions
  - How to add a strategy config via CLI or dashboard
  - How to run in signal-only mode vs. live trading mode

---

## P1 — Make It Tradeable

- [x] **Alpaca broker connector** (`ingest/src/connectors/alpaca.rs` or new `broker` module)
  - Implement `BrokerConnector` trait (or equivalent) for Alpaca REST API
  - Paper trading first — use `https://paper-api.alpaca.markets`
  - Operations needed: `submit_order`, `get_positions`, `get_account`, `cancel_order`
  - Sync Alpaca positions into `portfolio.positions` on startup and every 5 min during market hours
  - Store Alpaca API key/secret via env vars (`ALPACA_KEY_ID`, `ALPACA_SECRET_KEY`)

- [x] **Signal-to-order bridge**
  - Add `auto_execute: bool` field to `StrategyConfig` (default: `false`)
  - When a `TradeSignal` is emitted and `auto_execute = true`, submit order to configured broker
  - Add `execution_mode` to config: `signal_only` | `paper` | `live` — require explicit `live` to execute real orders
  - Log every order submission and fill to `portfolio.trades` with broker confirmation ID

- [x] **Notification system**
  - Add `notifications` section to `economind.toml`: webhook URL (Discord/Slack), email SMTP config
  - Send webhook POST when: signals are emitted, strategy run completes, position opened/closed, error occurs
  - Payload: structured JSON with signal details, timestamp, strategy name, score, rationale
  - Gracefully skip if no notification config is set

- [x] **Risk controls**
  - `max_drawdown_pct` config: pause new signal execution if portfolio is down more than X% from peak
  - `max_position_pct` config: cap any single position at X% of portfolio (enforced in sizer pipeline)
  - `max_open_positions` config: limit total concurrent open positions
  - `correlated_sector_limit` config: limit total exposure to any single GICS sector
  - Enforce these in the signal-to-order bridge before submitting; log when a signal is blocked by risk controls

---

## P2 — Dashboard & UI

- [ ] **Fix dashboard build pipeline**
  - Verify Node.js/npm is available in build environment
  - Add `ECONOMIND_DASHBOARD_PATH` env var support in `api/build.rs` (already in resolved decisions):
    - If set: serve from filesystem path (dev mode)
    - If unset: serve embedded assets (release mode)
  - Test that `cargo build --release` successfully embeds the built dashboard
  - Add `dashboard/build/` to `.gitignore` (it's a build artifact)

- [ ] **Dashboard: login / API key flow**
  - Verify the API key login screen works end-to-end
  - Test WebSocket reconnection on key change

- [ ] **Dashboard: universe management view**
  - Add "Universe" page or section to Strategy Manager
  - Show tracked symbols, sector breakdown, add/remove symbols

---

## P3 — Production Quality

- [ ] **TimescaleDB migration**
  - Add a new migration converting `market.bars` to a TimescaleDB hypertable
  - Partition on `(symbol, date)`, chunk interval 1 month
  - Add compression policy: compress chunks older than 1 year
  - Add `timescaledb` feature flag so the migration is skipped if the extension isn't installed
    (allows plain PostgreSQL for dev/test)

- [ ] **Integration test suite**
  - End-to-end test: seed 100 bars for 5 dummy symbols → run strategy pipeline → assert signals produced
  - Use `testcontainers` crate to spin up a real PostgreSQL instance in CI
  - Test each ingestion connector against recorded HTTP responses (wiremock or `httpmock`)
  - Test backtest runner: known bars + known strategy → assert known performance metrics

- [ ] **CI pipeline** (GitHub Actions)
  - `cargo check --workspace`
  - `cargo test --workspace` (with PostgreSQL testcontainer)
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check`
  - Dashboard: `npm run build` in `dashboard/`
  - Gate PRs on all checks passing

- [ ] **Clean up old crate directories**
  - Delete `server/`, `datamodel/`, `datafeed/`, `algorithms/` from disk
  - Verify `cargo build --workspace` still passes after deletion
  - Update any stale references in documentation

- [ ] **Performance baseline**
  - Benchmark strategy run time for 500-symbol universe (target: < 10 seconds)
  - Benchmark DuckDB sync time from PostgreSQL (target: < 5 seconds for 2 years of daily bars)
  - Add `economind bench` CLI subcommand or document manual benchmark procedure

---

## Deferred (Post-1.0)

- Interactive Brokers (IBKR) broker connector
- Dynamic strategy plugin loading (cdylib hot-reload)
- Paid data source connectors (Polygon.io, Alpha Vantage premium)
- Options data and strategies
- Intraday bar ingestion and intraday strategies
- Multi-user support (currently single-user by design)
- Mobile push notifications
- Strategy parameter auto-tuning (extend ensemble weight optimizer to all params)
- News sentiment scoring (integrate with existing `NewsStory` type)
