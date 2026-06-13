-- DuckDB schema for Economind (single embedded database — no PostgreSQL required)
--
-- All tables live here: market data, fundamentals, strategy configs, backtest
-- results, and live/paper portfolio positions.

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
    interval  TEXT      NOT NULL,
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
    fetched_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (series_id, date)
);

-- ── Strategy configs ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy_configs (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    name            TEXT    NOT NULL,
    description     TEXT,
    composition     TEXT    NOT NULL DEFAULT 'pipeline',
    plugins_json    TEXT    NOT NULL DEFAULT '[]',
    parameters_json TEXT    NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    auto_execute    BOOLEAN NOT NULL DEFAULT FALSE,
    execution_mode  TEXT    NOT NULL DEFAULT 'signal_only',
    version         INTEGER NOT NULL DEFAULT 1,
    created_at      TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMP NOT NULL DEFAULT NOW()
);

-- ── Strategy runs ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy_runs (
    id                   TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    config_id            TEXT NOT NULL,
    started_at           TIMESTAMP NOT NULL DEFAULT NOW(),
    completed_at         TIMESTAMP,
    status               TEXT NOT NULL DEFAULT 'running',
    signal_count         INTEGER NOT NULL DEFAULT 0,
    error_message        TEXT,
    config_snapshot_json TEXT NOT NULL DEFAULT '{}'
);

-- ── Strategy signals ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS strategy_signals (
    id                 TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    run_id             TEXT NOT NULL,
    config_id          TEXT NOT NULL,
    symbol             TEXT NOT NULL,
    direction          TEXT NOT NULL,
    identifier_score   DOUBLE NOT NULL DEFAULT 0.0,
    timing_score       DOUBLE NOT NULL DEFAULT 0.0,
    position_shares    DOUBLE,
    position_notional  DOUBLE,
    portfolio_fraction DOUBLE,
    rationale          TEXT,
    analysis_brief     TEXT,
    emitted_at         TIMESTAMP NOT NULL DEFAULT NOW()
);

-- ── Backtest runs ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest_runs (
    id                   TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    config_id            TEXT NOT NULL,
    config_snapshot_json TEXT NOT NULL DEFAULT '{}',
    from_date            DATE NOT NULL,
    to_date              DATE NOT NULL,
    initial_capital      DOUBLE NOT NULL,
    final_capital        DOUBLE,
    cagr                 DOUBLE,
    sharpe_ratio         DOUBLE,
    sortino_ratio        DOUBLE,
    max_drawdown         DOUBLE,
    max_drawdown_days    INTEGER,
    win_rate             DOUBLE,
    profit_factor        DOUBLE,
    expectancy           DOUBLE,
    total_trades         INTEGER,
    avg_hold_days        DOUBLE,
    status               TEXT NOT NULL DEFAULT 'running',
    started_at           TIMESTAMP NOT NULL DEFAULT NOW(),
    completed_at         TIMESTAMP,
    error_message        TEXT
);

-- ── Backtest trades ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest_trades (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    run_id      TEXT   NOT NULL,
    symbol      TEXT   NOT NULL,
    direction   TEXT   NOT NULL,
    entry_date  DATE   NOT NULL,
    entry_price DOUBLE NOT NULL,
    exit_date   DATE,
    exit_price  DOUBLE,
    shares      DOUBLE NOT NULL,
    gross_pnl   DOUBLE,
    commission  DOUBLE NOT NULL DEFAULT 0.0,
    net_pnl     DOUBLE,
    hold_days   INTEGER
);

-- ── Backtest equity curve ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS backtest_equity_curve (
    run_id          TEXT   NOT NULL,
    date            DATE   NOT NULL,
    portfolio_value DOUBLE NOT NULL,
    cash            DOUBLE NOT NULL,
    drawdown        DOUBLE NOT NULL DEFAULT 0.0,
    PRIMARY KEY (run_id, date)
);

-- ── Portfolio positions (live / paper) ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS portfolio_positions (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    symbol          TEXT   NOT NULL,
    shares          DOUBLE NOT NULL,
    entry_price     DOUBLE NOT NULL,
    entry_at        TIMESTAMP NOT NULL DEFAULT NOW(),
    status          TEXT   NOT NULL DEFAULT 'open',
    broker_order_id TEXT,
    exit_price      DOUBLE,
    exit_at         TIMESTAMP,
    realized_pnl    DOUBLE
);

-- ── Portfolio equity snapshots (for drawdown tracking) ────────────────────────

CREATE TABLE IF NOT EXISTS portfolio_equity (
    date            DATE PRIMARY KEY,
    portfolio_value DOUBLE NOT NULL,
    cash            DOUBLE NOT NULL DEFAULT 0.0,
    peak_value      DOUBLE NOT NULL
);

-- ── Agent chat sessions ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS chat_sessions (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    title       TEXT NOT NULL DEFAULT 'New chat',
    persona_id  TEXT,
    depth       TEXT,
    created_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS chat_messages (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    session_id  TEXT      NOT NULL,
    ordinal     INTEGER   NOT NULL,
    role        TEXT      NOT NULL,
    content     TEXT      NOT NULL,
    created_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, ordinal)
);

-- ── Runtime settings ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
