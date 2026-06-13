# Economind — Setup Guide

Economind is a low-frequency algorithmic trading platform that runs locally.
It requires PostgreSQL for durable storage and optionally DuckDB for fast analytics.

---

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Rust | stable (1.85+) | Install via [rustup](https://rustup.rs/) |
| PostgreSQL | 15 or 16 | Or use Docker (see below) |
| Node.js | 20+ | Only needed to rebuild the dashboard |

For the simplest setup, use Docker Compose — it handles PostgreSQL automatically.

---

## Option A — Docker Compose (recommended)

### 1. Clone and configure

```bash
git clone <repo-url> economind
cd economind

cp .env.example .env
```

Edit `.env` and set:
- `POSTGRES_PASSWORD` — any strong password
- `ECONOMIND_API_KEY` — generate with `openssl rand -hex 32`
- `FRED_API_KEY` — free registration at <https://fred.stlouisfed.org/docs/api/fred/>

### 2. Start services

```bash
docker compose up -d
```

The postgres container runs migrations automatically on first start via `docker/init.sql`.
The economind container applies sqlx migrations and starts the API server.

### 3. Load the ticker universe

```bash
docker compose exec economind economind universe load --file /app/universe.csv
```

Or from the host (if `economind` binary is on PATH):

```bash
economind universe load --file universe.csv
```

### 4. Verify

Open <http://localhost:8080/health> — you should see `{"status":"ok"}`.

The dashboard is at <http://localhost:8080>. Log in with the `ECONOMIND_API_KEY` you set.

---

## Option B — Local development (no Docker)

### 1. Set up PostgreSQL

```bash
createdb economind
```

### 2. Configure

```bash
cp .env.example .env
# Edit DATABASE_URL, ECONOMIND_API_KEY, FRED_API_KEY
cp economind.toml.example economind.toml
# Edit duckdb_path, scheduler times, etc.
```

Source the env file before running:

```bash
source .env   # bash/zsh
# or: set -a && . .env && set +a
```

On Windows (PowerShell):

```powershell
Get-Content .env | ForEach-Object {
    if ($_ -match '^([^#][^=]+)=(.*)$') { [System.Environment]::SetEnvironmentVariable($Matches[1].Trim(), $Matches[2].Trim()) }
}
```

Or install [dotenvy-cli](https://crates.io/crates/dotenvy-cli): `cargo install dotenvy-cli` then prefix commands with `dotenvy`.

### 3. Build and run

```bash
# Run the API server
cargo run --bin economind-serve

# Or use the CLI
cargo run --bin economind -- --help
```

### 4. Load the ticker universe

```bash
cargo run --bin economind -- universe load --file universe.csv
```

---

## API Keys

### FRED (Federal Reserve Economic Data) — required for macro ingestion

1. Register at <https://fred.stlouisfed.org/docs/api/fred/>
2. Copy your API key into `.env` as `FRED_API_KEY`

### SimFin — optional, for fundamental data

1. Register at <https://simfin.com/>
2. Copy your API key into `.env` as `SIMFIN_API_KEY`

### Anthropic — optional, for LLM-powered analysis via the MCP server

1. Get an API key at <https://console.anthropic.com/>
2. Set `ANTHROPIC_API_KEY` in `.env`

---

## First strategy run

### 1. Create a strategy configuration

Use the CLI or dashboard to add a strategy config.
Example via CLI (after `cargo run --bin economind -- --help`):

```bash
economind run --config <uuid>
```

Configs are stored in the `strategy.configs` table. You can also insert one directly:

```sql
INSERT INTO strategy.configs (name, enabled, pipeline_json)
VALUES ('momentum-test', true, '{"type":"momentum","lookback":20}');
```

### 2. Ingest historical bars

```bash
economind ingest bars --since 2024-01-01
```

This fetches daily OHLCV data for all symbols in the universe via Yahoo Finance.

### 3. Run the strategy

```bash
economind run --config <uuid> --lookback-days 365
```

Signals are stored in `strategy.signals` and visible in the dashboard.

---

## Signal-only vs. live trading

By default the platform runs in **signal-only** mode — it identifies trade opportunities
but does not submit orders. This is the safe default for all new installs.

To enable paper or live trading, see the P1 items in `TODO.md` (broker connector
and signal-to-order bridge — not yet implemented in this release).

---

## Scheduler

The background scheduler runs automatically when `economind-serve` starts
(unless `schedule.enabled = false` in `economind.toml`).

Default daily schedule (all times UTC):

| Job | Time | What it does |
|-----|------|-------------|
| Bar ingestion | 22:00 | Downloads recent OHLCV bars for all universe symbols |
| Macro refresh | 23:00 | Fetches FRED macro indicators |
| Fundamentals | Sun 23:00 | Downloads EDGAR filings + SimFin data |
| Strategy run | 23:30 | Runs all enabled strategy configs, emits signals |

Override times in `economind.toml` under `[schedule]` without restarting via env var overrides
(`ECONOMIND_SCHED_BARS_HH_MM`, `ECONOMIND_SCHED_MACRO_HH_MM`, etc.).

---

## Troubleshooting

**`DATABASE_URL must be set`** — ensure your `.env` is loaded and `DATABASE_URL` is set.

**`npm not found — skipping dashboard build`** — the dashboard won't be embedded in debug
builds unless Node.js is installed. Run `npm install && npm run build` in `dashboard/`
to build it, or use `cargo build --release` which will fail loudly if Node isn't found.

**File lock errors on Windows (OS error 32)** — the `target/` directory inside Dropbox
causes occasional lock conflicts. Move the project outside Dropbox or add `target/` to
Dropbox's excluded folders list.

**Yahoo Finance 429 errors** — reduce `ingest.bar_concurrency` in `economind.toml` to `2`
and add a longer delay between runs.
