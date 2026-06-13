-- Migration 001: Initial schema
-- Creates all schemas and migrates existing economind.* tables into the
-- new namespace structure defined in the implementation plan.
--
-- Safe to run against a fresh database OR an existing prototype database
-- that has the old economind.* tables — the CREATE TABLE IF NOT EXISTS and
-- INSERT ... SELECT patterns handle both cases.

BEGIN;

-- ── Extensions ────────────────────────────────────────────────────────────────

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS hstore;

-- TimescaleDB is required for hypertable support on market.bars.
-- Install with: https://docs.timescale.com/install/latest/
CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;

-- ── Schemas ───────────────────────────────────────────────────────────────────

CREATE SCHEMA IF NOT EXISTS market;
CREATE SCHEMA IF NOT EXISTS strategy;
CREATE SCHEMA IF NOT EXISTS portfolio;
CREATE SCHEMA IF NOT EXISTS backtest;
CREATE SCHEMA IF NOT EXISTS system;

-- Keep the old economind schema alive during migration; dropped in 002.
CREATE SCHEMA IF NOT EXISTS economind;

-- ── system.settings ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS system.settings (
    key   VARCHAR(200) PRIMARY KEY,
    value TEXT
);

COMMENT ON TABLE  system.settings       IS 'Platform-wide key-value configuration';
COMMENT ON COLUMN system.settings.key   IS 'Setting key (e.g. scheduler.bars_cron)';
COMMENT ON COLUMN system.settings.value IS 'Setting value (text; JSON allowed)';

-- Migrate any existing settings rows
INSERT INTO system.settings (key, value)
SELECT key, value FROM economind.settings
ON CONFLICT (key) DO NOTHING;

-- ── market.instruments ────────────────────────────────────────────────────────
-- Replaces economind.tickers with proper types and expanded columns.

CREATE TABLE IF NOT EXISTS market.instruments (
    symbol      VARCHAR(20)  NOT NULL,
    exchange    VARCHAR(20),
    name        VARCHAR(500),
    country     VARCHAR(50),
    industry    VARCHAR(200),
    sector      VARCHAR(200),
    ipo_year    SMALLINT,
    market_cap  NUMERIC(20, 4),
    description TEXT,
    active      BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (symbol)
);

COMMENT ON TABLE  market.instruments             IS 'Tracked instruments (equities, ETFs)';
COMMENT ON COLUMN market.instruments.symbol      IS 'Ticker symbol (e.g. AAPL)';
COMMENT ON COLUMN market.instruments.exchange    IS 'Primary exchange (e.g. NASDAQ)';
COMMENT ON COLUMN market.instruments.ipo_year    IS 'Year of IPO';
COMMENT ON COLUMN market.instruments.market_cap  IS 'Market capitalisation in USD';
COMMENT ON COLUMN market.instruments.active      IS 'False = delisted / removed from universe';

-- Migrate existing tickers
INSERT INTO market.instruments (symbol, exchange, name, country, industry, sector, market_cap, description)
SELECT
    symbol,
    exchange,
    name,
    country,
    industry,
    sector,
    marketcap::NUMERIC(20,4),
    description
FROM economind.tickers
ON CONFLICT (symbol) DO NOTHING;

-- ── market.bars ───────────────────────────────────────────────────────────────
-- Time-series OHLCV bars. Converted to a TimescaleDB hypertable in migration 002.

CREATE TABLE IF NOT EXISTS market.bars (
    symbol    VARCHAR(20)   NOT NULL,
    interval  VARCHAR(10)   NOT NULL,  -- '1d', '1h', '15m', '5m', '1m'
    time      TIMESTAMPTZ   NOT NULL,
    open      NUMERIC(16,6) NOT NULL,
    high      NUMERIC(16,6) NOT NULL,
    low       NUMERIC(16,6) NOT NULL,
    close     NUMERIC(16,6) NOT NULL,
    volume    BIGINT        NOT NULL DEFAULT 0,
    PRIMARY KEY (symbol, interval, time)
);

COMMENT ON TABLE  market.bars          IS 'OHLCV bars for all tracked instruments and intervals';
COMMENT ON COLUMN market.bars.symbol   IS 'Instrument symbol';
COMMENT ON COLUMN market.bars.interval IS 'Bar interval: 1d, 1h, 15m, 5m, 1m';
COMMENT ON COLUMN market.bars.time     IS 'Bar open timestamp (UTC)';

-- Migrate existing daily_candle rows as '1d' bars
INSERT INTO market.bars (symbol, interval, time, open, high, low, close, volume)
SELECT
    symbol,
    '1d',
    date::TIMESTAMPTZ,
    open::NUMERIC(16,6),
    high::NUMERIC(16,6),
    low::NUMERIC(16,6),
    close::NUMERIC(16,6),
    volume::BIGINT
FROM economind.daily_candle
ON CONFLICT (symbol, interval, time) DO NOTHING;

-- ── market.fundamentals ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS market.income_statements (
    symbol              VARCHAR(20)   NOT NULL,
    period_end          DATE          NOT NULL,
    period_type         VARCHAR(10)   NOT NULL DEFAULT 'annual',  -- 'annual', 'quarterly'
    revenue             NUMERIC(20,4),
    cogs                NUMERIC(20,4),
    gross_profit        NUMERIC(20,4),
    operating_income    NUMERIC(20,4),
    ebit                NUMERIC(20,4),
    net_income          NUMERIC(20,4),
    eps                 NUMERIC(12,6),
    interest_expense    NUMERIC(20,4),
    tax_expense         NUMERIC(20,4),
    fetched_at          TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS market.balance_sheets (
    symbol          VARCHAR(20)   NOT NULL,
    period_end      DATE          NOT NULL,
    period_type     VARCHAR(10)   NOT NULL DEFAULT 'annual',
    total_assets    NUMERIC(20,4),
    total_debt      NUMERIC(20,4),
    total_equity    NUMERIC(20,4),
    cash            NUMERIC(20,4),
    fetched_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS market.cash_flow_statements (
    symbol                VARCHAR(20)   NOT NULL,
    period_end            DATE          NOT NULL,
    period_type           VARCHAR(10)   NOT NULL DEFAULT 'annual',
    operating_cash_flow   NUMERIC(20,4),
    capex                 NUMERIC(20,4),
    free_cash_flow        NUMERIC(20,4),
    fetched_at            TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS market.dividends (
    symbol          VARCHAR(20)   NOT NULL,
    ex_date         DATE          NOT NULL,
    payment_date    DATE,
    amount          NUMERIC(12,6) NOT NULL,
    PRIMARY KEY (symbol, ex_date)
);

CREATE TABLE IF NOT EXISTS market.stock_splits (
    symbol  VARCHAR(20)   NOT NULL,
    date    DATE          NOT NULL,
    ratio   NUMERIC(12,6) NOT NULL,
    PRIMARY KEY (symbol, date)
);

-- ── market.macro_series ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS market.macro_series (
    series_id   VARCHAR(50)   NOT NULL,   -- FRED series ID, e.g. 'DGS10'
    date        DATE          NOT NULL,
    value       NUMERIC(20,8),
    fetched_at  TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (series_id, date)
);

COMMENT ON TABLE  market.macro_series           IS 'Macro time series from FRED and other sources';
COMMENT ON COLUMN market.macro_series.series_id IS 'FRED series ID (e.g. DGS10, CPIAUCSL, VIXCLS)';

-- ── market.news ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS market.news (
    id           UUID          NOT NULL DEFAULT gen_random_uuid(),
    symbol       VARCHAR(20),   -- NULL = market-wide news
    headline     TEXT          NOT NULL,
    summary      TEXT,
    story        TEXT,
    url          TEXT          UNIQUE,  -- deduplicate by URL when present
    evaluation   TEXT,
    published_at TIMESTAMPTZ,
    fetched_at   TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS news_symbol_published_idx
    ON market.news (symbol, published_at DESC);

-- ── strategy.configs ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy.configs (
    id              UUID          NOT NULL DEFAULT gen_random_uuid(),
    name            VARCHAR(200)  NOT NULL,
    description     TEXT,
    composition     VARCHAR(20)   NOT NULL DEFAULT 'pipeline',  -- 'pipeline', 'voting', 'ensemble'
    enabled         BOOLEAN       NOT NULL DEFAULT TRUE,
    parameters      JSONB         NOT NULL DEFAULT '{}',
    version         INTEGER       NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id)
);

COMMENT ON TABLE  strategy.configs             IS 'Versioned strategy plugin configurations';
COMMENT ON COLUMN strategy.configs.composition IS 'Composition mode: pipeline, voting, or ensemble';
COMMENT ON COLUMN strategy.configs.parameters  IS 'Strategy-specific parameters as JSON key-value map';
COMMENT ON COLUMN strategy.configs.version     IS 'Incremented on every parameter change for audit trail';

-- ── strategy.runs ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy.runs (
    id              UUID          NOT NULL DEFAULT gen_random_uuid(),
    config_id       UUID          NOT NULL REFERENCES strategy.configs(id),
    started_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,
    status          VARCHAR(20)   NOT NULL DEFAULT 'running',  -- 'running', 'completed', 'failed'
    signal_count    INTEGER       NOT NULL DEFAULT 0,
    error_message   TEXT,
    config_snapshot JSONB         NOT NULL DEFAULT '{}',  -- parameters as-of this run
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS strategy_runs_config_idx ON strategy.runs (config_id, started_at DESC);

-- ── strategy.signals ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy.signals (
    id                  UUID          NOT NULL DEFAULT gen_random_uuid(),
    run_id              UUID          NOT NULL REFERENCES strategy.runs(id),
    config_id           UUID          NOT NULL REFERENCES strategy.configs(id),
    symbol              VARCHAR(20)   NOT NULL,
    direction           VARCHAR(10)   NOT NULL,   -- 'long', 'short'
    identifier_score    NUMERIC(6,4)  NOT NULL,
    timing_score        NUMERIC(6,4)  NOT NULL,
    position_shares     NUMERIC(16,4),
    position_notional   NUMERIC(20,4),
    portfolio_fraction  NUMERIC(8,6),
    rationale           TEXT,
    analysis_brief      TEXT,          -- populated by agentic layer (Phase 7)
    emitted_at          TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS signals_symbol_emitted_idx ON strategy.signals (symbol, emitted_at DESC);
CREATE INDEX IF NOT EXISTS signals_run_idx            ON strategy.signals (run_id);

-- ── portfolio.positions ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS portfolio.positions (
    id              UUID          NOT NULL DEFAULT gen_random_uuid(),
    symbol          VARCHAR(20)   NOT NULL,
    signal_id       UUID          REFERENCES strategy.signals(id),
    direction       VARCHAR(10)   NOT NULL,   -- 'long', 'short'
    shares          NUMERIC(16,4) NOT NULL,
    entry_price     NUMERIC(16,6) NOT NULL,
    entry_at        TIMESTAMPTZ   NOT NULL,
    exit_price      NUMERIC(16,6),
    exit_at         TIMESTAMPTZ,
    status          VARCHAR(10)   NOT NULL DEFAULT 'open',  -- 'open', 'closed'
    realized_pnl    NUMERIC(20,4),
    notes           TEXT,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS positions_symbol_status_idx ON portfolio.positions (symbol, status);
CREATE INDEX IF NOT EXISTS positions_entry_idx         ON portfolio.positions (entry_at DESC);

-- ── portfolio.trades ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS portfolio.trades (
    id              UUID          NOT NULL DEFAULT gen_random_uuid(),
    position_id     UUID          NOT NULL REFERENCES portfolio.positions(id),
    symbol          VARCHAR(20)   NOT NULL,
    side            VARCHAR(10)   NOT NULL,   -- 'buy', 'sell'
    shares          NUMERIC(16,4) NOT NULL,
    price           NUMERIC(16,6) NOT NULL,
    commission      NUMERIC(12,4) NOT NULL DEFAULT 0,
    broker_order_id VARCHAR(100),
    executed_at     TIMESTAMPTZ   NOT NULL,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS trades_position_idx ON portfolio.trades (position_id);
CREATE INDEX IF NOT EXISTS trades_executed_idx ON portfolio.trades (executed_at DESC);

-- ── backtest.runs ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest.runs (
    id                  UUID          NOT NULL DEFAULT gen_random_uuid(),
    config_id           UUID          NOT NULL REFERENCES strategy.configs(id),
    config_snapshot     JSONB         NOT NULL DEFAULT '{}',
    from_date           DATE          NOT NULL,
    to_date             DATE          NOT NULL,
    initial_capital     NUMERIC(20,4) NOT NULL,
    final_capital       NUMERIC(20,4),
    -- Performance metrics
    cagr                NUMERIC(10,6),
    sharpe_ratio        NUMERIC(10,6),
    sortino_ratio       NUMERIC(10,6),
    max_drawdown        NUMERIC(8,6),
    max_drawdown_days   INTEGER,
    win_rate            NUMERIC(8,6),
    profit_factor       NUMERIC(10,4),
    expectancy          NUMERIC(16,4),
    total_trades        INTEGER,
    avg_hold_days       NUMERIC(10,2),
    -- Run metadata
    status              VARCHAR(20)   NOT NULL DEFAULT 'running',
    started_at          TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ,
    error_message       TEXT,
    PRIMARY KEY (id)
);

-- ── backtest.trades ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest.trades (
    id              UUID          NOT NULL DEFAULT gen_random_uuid(),
    run_id          UUID          NOT NULL REFERENCES backtest.runs(id),
    symbol          VARCHAR(20)   NOT NULL,
    direction       VARCHAR(10)   NOT NULL,
    entry_date      DATE          NOT NULL,
    entry_price     NUMERIC(16,6) NOT NULL,
    exit_date       DATE,
    exit_price      NUMERIC(16,6),
    shares          NUMERIC(16,4) NOT NULL,
    gross_pnl       NUMERIC(20,4),
    commission      NUMERIC(12,4) NOT NULL DEFAULT 0,
    net_pnl         NUMERIC(20,4),
    hold_days       INTEGER,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS backtest_trades_run_idx ON backtest.trades (run_id);

-- ── backtest.equity_curve ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest.equity_curve (
    run_id          UUID          NOT NULL REFERENCES backtest.runs(id),
    date            DATE          NOT NULL,
    portfolio_value NUMERIC(20,4) NOT NULL,
    cash            NUMERIC(20,4) NOT NULL,
    drawdown        NUMERIC(8,6)  NOT NULL DEFAULT 0,
    PRIMARY KEY (run_id, date)
);

-- ── system.audit_log ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS system.audit_log (
    id          BIGSERIAL     NOT NULL,
    event_type  VARCHAR(100)  NOT NULL,
    entity_type VARCHAR(100),
    entity_id   UUID,
    payload     JSONB         NOT NULL DEFAULT '{}',
    occurred_at TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id)
);

COMMENT ON TABLE system.audit_log IS 'Immutable append-only event log — no updates or deletes permitted';

-- Prevent UPDATE and DELETE on audit_log
CREATE OR REPLACE FUNCTION system.audit_log_immutable()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is immutable — updates and deletes are not permitted';
END;
$$;

DROP TRIGGER IF EXISTS audit_log_no_update ON system.audit_log;
CREATE TRIGGER audit_log_no_update
    BEFORE UPDATE OR DELETE ON system.audit_log
    FOR EACH ROW EXECUTE FUNCTION system.audit_log_immutable();

-- ── system.scheduled_jobs ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS system.scheduled_jobs (
    id          UUID          NOT NULL DEFAULT gen_random_uuid(),
    name        VARCHAR(200)  NOT NULL UNIQUE,
    description TEXT,
    cron        VARCHAR(100)  NOT NULL,   -- cron expression, e.g. '0 18 * * 1-5'
    enabled     BOOLEAN       NOT NULL DEFAULT TRUE,
    last_run_at TIMESTAMPTZ,
    last_status VARCHAR(20),
    PRIMARY KEY (id)
);

-- Seed default scheduled jobs
INSERT INTO system.scheduled_jobs (name, description, cron, enabled) VALUES
    ('ingest.bars',           'Fetch prior-day OHLCV bars for all tracked instruments', '0 17 * * 1-5', TRUE),
    ('ingest.fundamentals',   'Refresh fundamental data from EDGAR and SimFin',          '0 18 * * 0',   TRUE),
    ('ingest.macro',          'Pull latest FRED macro series',                           '0 18 * * 1-5', TRUE),
    ('strategy.run',          'Run all enabled strategy stacks and emit signals',        '30 18 * * 1-5',TRUE),
    ('db.duckdb_sync',        'Sync DuckDB analytical snapshot from PostgreSQL',         '15 18 * * 1-5',TRUE),
    ('broker.sync',           'Sync positions and fills from broker API',                '*/5 13-21 * * 1-5', FALSE)
ON CONFLICT (name) DO NOTHING;

COMMIT;
