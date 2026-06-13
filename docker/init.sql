-- docker/init.sql
-- Runs once on first postgres container start (via docker-entrypoint-initdb.d).
-- The Economind platform applies all schema migrations via sqlx on startup,
-- so this file only needs to bootstrap prerequisite extensions.

-- TimescaleDB extension (already installed in the timescale/timescaledb image).
CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;

-- pgcrypto for gen_random_uuid() on older PostgreSQL versions.
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Log startup for visibility in docker compose logs.
DO $$
BEGIN
  RAISE NOTICE 'Economind database initialized. Migrations will run on first platform start.';
END;
$$;
