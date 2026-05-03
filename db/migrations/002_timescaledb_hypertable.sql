-- Migration 002: Convert market.bars to a TimescaleDB hypertable
--
-- Must run AFTER migration 001 and AFTER the TimescaleDB extension is installed.
-- If TimescaleDB is not available this migration will fail — install it first:
--   https://docs.timescale.com/install/latest/
--
-- Chunk interval of 1 month balances query performance vs. number of chunks
-- for daily/hourly data over multi-year history.

BEGIN;

SELECT create_hypertable(
    'market.bars',
    'time',
    chunk_time_interval => INTERVAL '1 month',
    if_not_exists       => TRUE
);

-- Compression policy: compress chunks older than 3 months.
-- This dramatically reduces disk usage for historical bar data.
ALTER TABLE market.bars SET (
    timescaledb.compress,
    timescaledb.compress_orderby   = 'time DESC',
    timescaledb.compress_segmentby = 'symbol, interval'
);

SELECT add_compression_policy('market.bars', INTERVAL '3 months', if_not_exists => TRUE);

-- Retention policy: keep all data (no automatic drop).
-- Uncomment and adjust if you want automatic pruning:
-- SELECT add_retention_policy('market.bars', INTERVAL '10 years', if_not_exists => TRUE);

COMMIT;
