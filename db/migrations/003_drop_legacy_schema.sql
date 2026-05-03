-- Migration 003: Drop the legacy economind.* schema
--
-- Safe to run only after verifying all data has been migrated by migration 001.
-- The old tables (economind.tickers, economind.daily_candle, economind.ticker_stats,
-- economind.settings) are superseded by the new schema namespaces.
--
-- Run this manually after confirming migration 001 completed cleanly.

BEGIN;

DROP TABLE IF EXISTS economind.daily_candle  CASCADE;
DROP TABLE IF EXISTS economind.ticker_stats  CASCADE;
DROP TABLE IF EXISTS economind.tickers       CASCADE;
DROP TABLE IF EXISTS economind.settings      CASCADE;
DROP SCHEMA IF EXISTS economind              CASCADE;

COMMIT;
