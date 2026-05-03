-- DuckDB analytical schema for Economind
--
-- This schema mirrors a subset of the PostgreSQL operational schema
-- for fast in-process analytics during strategy runs and backtesting.
-- Updated by db.duckdb_sync scheduled job (nightly, post-close).

-- ── Instruments ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS instruments (
    symbol      TEXT PRIMARY KEY,
    exchange    TEXT,
    name        TEXT,
    country     TEXT,
    industry    TEXT,
    sector      TEXT,
    ipoyear     TEXT,
    marketcap   DOUBLE,
    description TEXT,
    active      BOOLEAN NOT NULL DEFAULT TRUE
);

-- ── OHLCV bars ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS bars (
    symbol    TEXT      NOT NULL,
    interval  TEXT      NOT NULL,  -- '1d', '1h', '15m', '5m', '1m'
    time      TIMESTAMP NOT NULL,
    open      DOUBLE    NOT NULL,
    high      DOUBLE    NOT NULL,
    low       DOUBLE    NOT NULL,
    close     DOUBLE    NOT NULL,
    volume    BIGINT    NOT NULL DEFAULT 0,
    PRIMARY KEY (symbol, interval, time)
);

-- ── Fundamentals ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS income_statements (
    symbol            TEXT    NOT NULL,
    period_end        DATE    NOT NULL,
    period_type       TEXT    NOT NULL DEFAULT 'annual',
    revenue           DOUBLE,
    cogs              DOUBLE,
    gross_profit      DOUBLE,
    operating_income  DOUBLE,
    ebit              DOUBLE,
    net_income        DOUBLE,
    eps               DOUBLE,
    interest_expense  DOUBLE,
    tax_expense       DOUBLE,
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS balance_sheets (
    symbol        TEXT  NOT NULL,
    period_end    DATE  NOT NULL,
    period_type   TEXT  NOT NULL DEFAULT 'annual',
    total_assets  DOUBLE,
    total_debt    DOUBLE,
    total_equity  DOUBLE,
    cash          DOUBLE,
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS cash_flow_statements (
    symbol                TEXT  NOT NULL,
    period_end            DATE  NOT NULL,
    period_type           TEXT  NOT NULL DEFAULT 'annual',
    operating_cash_flow   DOUBLE,
    capex                 DOUBLE,
    free_cash_flow        DOUBLE,
    PRIMARY KEY (symbol, period_end, period_type)
);

CREATE TABLE IF NOT EXISTS dividends (
    symbol        TEXT   NOT NULL,
    ex_date       DATE   NOT NULL,
    payment_date  DATE,
    amount        DOUBLE NOT NULL,
    PRIMARY KEY (symbol, ex_date)
);

CREATE TABLE IF NOT EXISTS stock_splits (
    symbol  TEXT   NOT NULL,
    date    DATE   NOT NULL,
    ratio   DOUBLE NOT NULL,
    PRIMARY KEY (symbol, date)
);

-- ── News ─────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS news (
    id           TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    symbol       TEXT,
    headline     TEXT NOT NULL,
    summary      TEXT,
    story        TEXT,
    url          TEXT UNIQUE,
    evaluation   TEXT,
    published_at TIMESTAMP,
    fetched_at   TIMESTAMP NOT NULL DEFAULT NOW()
);

-- ── Macro series ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS macro_series (
    series_id   TEXT   NOT NULL,
    date        DATE   NOT NULL,
    value       DOUBLE,
    PRIMARY KEY (series_id, date)
);
