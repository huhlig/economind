-- Migration 004: Broker execution support
-- Adds execution control fields to strategy.configs.

BEGIN;

-- ── strategy.configs — execution control ──────────────────────────────────────

ALTER TABLE strategy.configs
    ADD COLUMN IF NOT EXISTS auto_execute    BOOLEAN     NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS execution_mode  VARCHAR(20) NOT NULL DEFAULT 'signal_only';

COMMENT ON COLUMN strategy.configs.auto_execute   IS 'If true, submit orders to broker when signals are emitted';
COMMENT ON COLUMN strategy.configs.execution_mode IS 'signal_only | paper | live';

-- ── portfolio.positions — broker tracking fields ───────────────────────────────

ALTER TABLE portfolio.positions
    ADD COLUMN IF NOT EXISTS broker_order_id VARCHAR(100),
    ADD COLUMN IF NOT EXISTS execution_mode  VARCHAR(20) NOT NULL DEFAULT 'signal_only';

COMMENT ON COLUMN portfolio.positions.broker_order_id IS 'Broker-assigned order ID for the entry fill';
COMMENT ON COLUMN portfolio.positions.execution_mode  IS 'Execution mode at time of entry';

-- ── portfolio.trades — broker tracking fields ─────────────────────────────────

ALTER TABLE portfolio.trades
    ADD COLUMN IF NOT EXISTS execution_mode VARCHAR(20) NOT NULL DEFAULT 'signal_only';

COMMENT ON COLUMN portfolio.trades.execution_mode IS 'paper or live';

COMMIT;
