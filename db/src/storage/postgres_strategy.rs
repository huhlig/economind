//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! PostgreSQL implementations of `StrategyStorage`, `MacroStorage`,
//! `PortfolioStorage`, and `BacktestStorage` (Phases 2 & 4).

use crate::{StorageError, StorageResult};
use crate::storage::postgres::PostgresStorage;
use crate::storage::strategy_traits::{
    BacktestRunRow, BacktestStorage, BacktestTradeRow, EquityCurvePoint,
    MacroSeriesPoint, MacroStorage, OpenPosition, PortfolioState, PortfolioStorage,
    StrategyConfigRow, StrategyRunRow, StrategySignalRow, StrategyStorage,
};
use chrono::{DateTime, NaiveDate, Utc};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use sqlx::Row;
use uuid::Uuid;

// ── MacroStorage ──────────────────────────────────────────────────────────────

impl MacroStorage for PostgresStorage {
    async fn upsert_macro_series(&self, points: &[MacroSeriesPoint]) -> StorageResult<()> {
        if points.is_empty() {
            return Ok(());
        }
        for p in points {
            sqlx::query(
                r#"
                INSERT INTO market.macro_series (series_id, date, value, fetched_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (series_id, date) DO UPDATE
                    SET value      = EXCLUDED.value,
                        fetched_at = EXCLUDED.fetched_at
                "#,
            )
            .bind(&p.series_id)
            .bind(p.date)
            .bind(p.value)
            .bind(p.fetched_at)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    async fn get_latest_macro_values(
        &self,
        series_ids: &[&str],
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        if series_ids.is_empty() {
            return Ok(vec![]);
        }

        // Fetch the latest row per series_id using DISTINCT ON.
        let ids: Vec<String> = series_ids.iter().map(|s| s.to_string()).collect();
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT ON (series_id)
                series_id, date, value, fetched_at
            FROM market.macro_series
            WHERE series_id = ANY($1)
            ORDER BY series_id, date DESC
            "#,
        )
        .bind(&ids)
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| MacroSeriesPoint {
                series_id: row.get("series_id"),
                date: row.get("date"),
                value: row.get("value"),
                fetched_at: row.get("fetched_at"),
            })
            .collect())
    }

    async fn query_macro_series(
        &self,
        series_id: &str,
        date_range: std::ops::Range<NaiveDate>,
    ) -> StorageResult<Vec<MacroSeriesPoint>> {
        let rows = sqlx::query(
            r#"
            SELECT series_id, date, value, fetched_at
            FROM market.macro_series
            WHERE series_id = $1 AND date >= $2 AND date < $3
            ORDER BY date ASC
            "#,
        )
        .bind(series_id)
        .bind(date_range.start)
        .bind(date_range.end)
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| MacroSeriesPoint {
                series_id: row.get("series_id"),
                date: row.get("date"),
                value: row.get("value"),
                fetched_at: row.get("fetched_at"),
            })
            .collect())
    }
}

// ── PortfolioStorage ──────────────────────────────────────────────────────────

impl PortfolioStorage for PostgresStorage {
    async fn load_portfolio_state(&self) -> StorageResult<PortfolioState> {
        // Load open positions.
        let rows = sqlx::query(
            r#"
            SELECT id, symbol, shares, entry_price, entry_at
            FROM portfolio.positions
            WHERE status = 'open'
            ORDER BY entry_at DESC
            "#,
        )
        .fetch_all(self.pool())
        .await?;

        let open_positions: Vec<OpenPosition> = rows
            .into_iter()
            .map(|row| OpenPosition {
                id: row.get("id"),
                symbol: Symbol::new(row.get::<&str, _>("symbol")),
                shares: row.get("shares"),
                entry_price: row.get("entry_price"),
                entry_at: row.get("entry_at"),
            })
            .collect();

        // Read portfolio_value and available_cash from system.settings.
        // Default to sensible zero values if not yet configured.
        let portfolio_value = self
            .get_setting_decimal("portfolio.total_value")
            .await
            .unwrap_or(Decimal::ZERO);
        let available_cash = self
            .get_setting_decimal("portfolio.available_cash")
            .await
            .unwrap_or(Decimal::ZERO);
        let current_drawdown = self
            .get_setting_decimal("portfolio.current_drawdown")
            .await
            .unwrap_or(Decimal::ZERO);

        Ok(PortfolioState {
            open_positions,
            portfolio_value,
            available_cash,
            current_drawdown,
        })
    }
}

impl PostgresStorage {
    /// Helper: read a single Decimal value from `system.settings`.
    async fn get_setting_decimal(&self, key: &str) -> StorageResult<Decimal> {
        let row = sqlx::query(
            "SELECT value FROM system.settings WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(r) => {
                let val: String = r.get("value");
                val.parse::<Decimal>()
                    .map_err(|e| StorageError::Provider(e.to_string()))
            }
            None => Err(StorageError::NotFound),
        }
    }
}

// ── StrategyStorage ───────────────────────────────────────────────────────────

impl StrategyStorage for PostgresStorage {
    // ── Configs ───────────────────────────────────────────────────────────────

    async fn insert_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy.configs
                (id, name, description, composition, parameters, enabled, version, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8, $9)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(row.id)
        .bind(&row.name)
        .bind(&row.description)
        .bind(&row.composition)
        .bind(&row.parameters_json)
        .bind(row.enabled)
        .bind(row.version as i32)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_strategy_config(&self, id: Uuid) -> StorageResult<Option<StrategyConfigRow>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, composition, parameters::text as parameters_json,
                   enabled, version, created_at, updated_at
            FROM strategy.configs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;

        Ok(row.map(config_row_from_pg))
    }

    async fn list_strategy_configs(&self) -> StorageResult<Vec<StrategyConfigRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, composition, parameters::text as parameters_json,
                   enabled, version, created_at, updated_at
            FROM strategy.configs
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(self.pool())
        .await?;

        Ok(rows.into_iter().map(config_row_from_pg).collect())
    }

    async fn update_strategy_config(&self, row: &StrategyConfigRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            UPDATE strategy.configs
            SET name        = $2,
                description = $3,
                composition = $4,
                parameters  = $5::jsonb,
                enabled     = $6,
                version     = $7,
                updated_at  = $8
            WHERE id = $1
            "#,
        )
        .bind(row.id)
        .bind(&row.name)
        .bind(&row.description)
        .bind(&row.composition)
        .bind(&row.parameters_json)
        .bind(row.enabled)
        .bind(row.version as i32)
        .bind(row.updated_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    // ── Runs ──────────────────────────────────────────────────────────────────

    async fn insert_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy.runs
                (id, config_id, started_at, status, signal_count, config_snapshot)
            VALUES ($1, $2, $3, $4, $5, $6::jsonb)
            "#,
        )
        .bind(row.id)
        .bind(row.config_id)
        .bind(row.started_at)
        .bind(&row.status)
        .bind(row.signal_count)
        .bind(&row.config_snapshot_json)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn complete_strategy_run(&self, row: &StrategyRunRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            UPDATE strategy.runs
            SET completed_at   = $2,
                status         = $3,
                signal_count   = $4,
                error_message  = $5
            WHERE id = $1
            "#,
        )
        .bind(row.id)
        .bind(row.completed_at)
        .bind(&row.status)
        .bind(row.signal_count)
        .bind(&row.error_message)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_strategy_run(&self, id: Uuid) -> StorageResult<Option<StrategyRunRow>> {
        let row = sqlx::query(
            r#"
            SELECT id, config_id, started_at, completed_at, status, signal_count,
                   error_message, config_snapshot::text as config_snapshot_json
            FROM strategy.runs WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;

        Ok(row.map(run_row_from_pg))
    }

    async fn list_strategy_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategyRunRow>> {
        let lim = limit.unwrap_or(100) as i64;
        let rows = match config_id {
            Some(cid) => sqlx::query(
                r#"
                SELECT id, config_id, started_at, completed_at, status, signal_count,
                       error_message, config_snapshot::text as config_snapshot_json
                FROM strategy.runs
                WHERE config_id = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
            )
            .bind(cid)
            .bind(lim)
            .fetch_all(self.pool())
            .await?,
            None => sqlx::query(
                r#"
                SELECT id, config_id, started_at, completed_at, status, signal_count,
                       error_message, config_snapshot::text as config_snapshot_json
                FROM strategy.runs
                ORDER BY started_at DESC
                LIMIT $1
                "#,
            )
            .bind(lim)
            .fetch_all(self.pool())
            .await?,
        };

        Ok(rows.into_iter().map(run_row_from_pg).collect())
    }

    // ── Signals ───────────────────────────────────────────────────────────────

    async fn insert_strategy_signals(&self, rows: &[StrategySignalRow]) -> StorageResult<()> {
        for row in rows {
            sqlx::query(
                r#"
                INSERT INTO strategy.signals
                    (id, run_id, config_id, symbol, direction,
                     identifier_score, timing_score,
                     position_shares, position_notional, portfolio_fraction,
                     rationale, analysis_brief, emitted_at)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                ON CONFLICT (id) DO NOTHING
                "#,
            )
            .bind(row.id)
            .bind(row.run_id)
            .bind(row.config_id)
            .bind(&row.symbol)
            .bind(&row.direction)
            .bind(row.identifier_score)
            .bind(row.timing_score)
            .bind(row.position_shares)
            .bind(row.position_notional)
            .bind(row.portfolio_fraction)
            .bind(&row.rationale)
            .bind(&row.analysis_brief)
            .bind(row.emitted_at)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    async fn query_strategy_signals(
        &self,
        run_id: Option<Uuid>,
        config_id: Option<Uuid>,
        symbol: Option<&Symbol>,
        since: Option<NaiveDate>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<StrategySignalRow>> {
        // Build query dynamically based on filters.
        // Using a simple approach with individual optional clauses appended.
        let mut conditions: Vec<String> = Vec::new();
        let mut param_idx = 1usize;

        if run_id.is_some() {
            conditions.push(format!("run_id = ${param_idx}"));
            param_idx += 1;
        }
        if config_id.is_some() {
            conditions.push(format!("config_id = ${param_idx}"));
            param_idx += 1;
        }
        if symbol.is_some() {
            conditions.push(format!("symbol = ${param_idx}"));
            param_idx += 1;
        }
        if since.is_some() {
            conditions.push(format!("emitted_at >= ${param_idx}"));
            param_idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let lim = limit.unwrap_or(500) as i64;
        let sql = format!(
            r#"
            SELECT id, run_id, config_id, symbol, direction,
                   identifier_score, timing_score,
                   position_shares, position_notional, portfolio_fraction,
                   rationale, analysis_brief, emitted_at
            FROM strategy.signals
            {where_clause}
            ORDER BY emitted_at DESC
            LIMIT ${param_idx}
            "#
        );

        // Bind parameters in order.
        let mut q = sqlx::query(&sql);
        if let Some(rid) = run_id { q = q.bind(rid); }
        if let Some(cid) = config_id { q = q.bind(cid); }
        if let Some(sym) = symbol { q = q.bind(sym.as_str()); }
        if let Some(s) = since {
            let dt: DateTime<Utc> = chrono::DateTime::from_naive_utc_and_offset(
                s.and_hms_opt(0, 0, 0).unwrap(), Utc,
            );
            q = q.bind(dt);
        }
        q = q.bind(lim);

        let rows = q.fetch_all(self.pool()).await?;
        Ok(rows.into_iter().map(signal_row_from_pg).collect())
    }

    async fn get_strategy_signal(&self, id: Uuid) -> StorageResult<Option<StrategySignalRow>> {
        let row = sqlx::query(
            r#"
            SELECT id, run_id, config_id, symbol, direction,
                   identifier_score, timing_score,
                   position_shares, position_notional, portfolio_fraction,
                   rationale, analysis_brief, emitted_at
            FROM strategy.signals WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;

        Ok(row.map(signal_row_from_pg))
    }
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn config_row_from_pg(row: sqlx::postgres::PgRow) -> StrategyConfigRow {
    StrategyConfigRow {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        composition: row.get("composition"),
        plugins_json: "[]".to_string(), // plugins stored separately in Phase 5
        parameters_json: row.get("parameters_json"),
        enabled: row.get("enabled"),
        version: row.get::<i32, _>("version") as u32,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn run_row_from_pg(row: sqlx::postgres::PgRow) -> StrategyRunRow {
    StrategyRunRow {
        id: row.get("id"),
        config_id: row.get("config_id"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        status: row.get("status"),
        signal_count: row.get("signal_count"),
        error_message: row.get("error_message"),
        config_snapshot_json: row.get("config_snapshot_json"),
    }
}

fn signal_row_from_pg(row: sqlx::postgres::PgRow) -> StrategySignalRow {
    StrategySignalRow {
        id: row.get("id"),
        run_id: row.get("run_id"),
        config_id: row.get("config_id"),
        symbol: row.get("symbol"),
        direction: row.get("direction"),
        identifier_score: row.get("identifier_score"),
        timing_score: row.get("timing_score"),
        position_shares: row.get("position_shares"),
        position_notional: row.get("position_notional"),
        portfolio_fraction: row.get("portfolio_fraction"),
        rationale: row.get("rationale"),
        analysis_brief: row.get("analysis_brief"),
        emitted_at: row.get("emitted_at"),
    }
}

// ── BacktestStorage ───────────────────────────────────────────────────────────

impl BacktestStorage for PostgresStorage {
    async fn insert_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO backtest.runs
                (id, config_id, config_snapshot, from_date, to_date,
                 initial_capital, status, started_at)
            VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(row.id)
        .bind(row.config_id)
        .bind(&row.config_snapshot_json)
        .bind(row.from_date)
        .bind(row.to_date)
        .bind(row.initial_capital)
        .bind(&row.status)
        .bind(row.started_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn complete_backtest_run(&self, row: &BacktestRunRow) -> StorageResult<()> {
        sqlx::query(
            r#"
            UPDATE backtest.runs
            SET final_capital      = $2,
                cagr               = $3,
                sharpe_ratio       = $4,
                sortino_ratio      = $5,
                max_drawdown       = $6,
                max_drawdown_days  = $7,
                win_rate           = $8,
                profit_factor      = $9,
                expectancy         = $10,
                total_trades       = $11,
                avg_hold_days      = $12,
                status             = $13,
                completed_at       = $14,
                error_message      = $15
            WHERE id = $1
            "#,
        )
        .bind(row.id)
        .bind(row.final_capital)
        .bind(row.cagr)
        .bind(row.sharpe_ratio)
        .bind(row.sortino_ratio)
        .bind(row.max_drawdown)
        .bind(row.max_drawdown_days)
        .bind(row.win_rate)
        .bind(row.profit_factor)
        .bind(row.expectancy)
        .bind(row.total_trades)
        .bind(row.avg_hold_days)
        .bind(&row.status)
        .bind(row.completed_at)
        .bind(&row.error_message)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get_backtest_run(&self, id: Uuid) -> StorageResult<Option<BacktestRunRow>> {
        let row = sqlx::query(
            r#"
            SELECT id, config_id, config_snapshot::text AS config_snapshot_json,
                   from_date, to_date, initial_capital, final_capital,
                   cagr, sharpe_ratio, sortino_ratio,
                   max_drawdown, max_drawdown_days,
                   win_rate, profit_factor, expectancy,
                   total_trades, avg_hold_days,
                   status, started_at, completed_at, error_message
            FROM backtest.runs WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;

        Ok(row.map(backtest_run_from_pg))
    }

    async fn list_backtest_runs(
        &self,
        config_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> StorageResult<Vec<BacktestRunRow>> {
        let lim = limit.unwrap_or(50) as i64;
        let rows = match config_id {
            Some(cid) => sqlx::query(
                r#"
                SELECT id, config_id, config_snapshot::text AS config_snapshot_json,
                       from_date, to_date, initial_capital, final_capital,
                       cagr, sharpe_ratio, sortino_ratio,
                       max_drawdown, max_drawdown_days,
                       win_rate, profit_factor, expectancy,
                       total_trades, avg_hold_days,
                       status, started_at, completed_at, error_message
                FROM backtest.runs
                WHERE config_id = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
            )
            .bind(cid)
            .bind(lim)
            .fetch_all(self.pool())
            .await?,
            None => sqlx::query(
                r#"
                SELECT id, config_id, config_snapshot::text AS config_snapshot_json,
                       from_date, to_date, initial_capital, final_capital,
                       cagr, sharpe_ratio, sortino_ratio,
                       max_drawdown, max_drawdown_days,
                       win_rate, profit_factor, expectancy,
                       total_trades, avg_hold_days,
                       status, started_at, completed_at, error_message
                FROM backtest.runs
                ORDER BY started_at DESC
                LIMIT $1
                "#,
            )
            .bind(lim)
            .fetch_all(self.pool())
            .await?,
        };

        Ok(rows.into_iter().map(backtest_run_from_pg).collect())
    }

    async fn insert_backtest_trades(&self, rows: &[BacktestTradeRow]) -> StorageResult<()> {
        for row in rows {
            sqlx::query(
                r#"
                INSERT INTO backtest.trades
                    (id, run_id, symbol, direction,
                     entry_date, entry_price, exit_date, exit_price,
                     shares, gross_pnl, commission, net_pnl, hold_days)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                ON CONFLICT (id) DO NOTHING
                "#,
            )
            .bind(row.id)
            .bind(row.run_id)
            .bind(&row.symbol)
            .bind(&row.direction)
            .bind(row.entry_date)
            .bind(row.entry_price)
            .bind(row.exit_date)
            .bind(row.exit_price)
            .bind(row.shares)
            .bind(row.gross_pnl)
            .bind(row.commission)
            .bind(row.net_pnl)
            .bind(row.hold_days)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    async fn get_backtest_trades(&self, run_id: Uuid) -> StorageResult<Vec<BacktestTradeRow>> {
        let rows = sqlx::query(
            r#"
            SELECT id, run_id, symbol, direction,
                   entry_date, entry_price, exit_date, exit_price,
                   shares, gross_pnl, commission, net_pnl, hold_days
            FROM backtest.trades
            WHERE run_id = $1
            ORDER BY entry_date ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;

        Ok(rows.into_iter().map(backtest_trade_from_pg).collect())
    }

    async fn insert_equity_curve(&self, points: &[EquityCurvePoint]) -> StorageResult<()> {
        for p in points {
            sqlx::query(
                r#"
                INSERT INTO backtest.equity_curve
                    (run_id, date, portfolio_value, cash, drawdown)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (run_id, date) DO UPDATE
                    SET portfolio_value = EXCLUDED.portfolio_value,
                        cash            = EXCLUDED.cash,
                        drawdown        = EXCLUDED.drawdown
                "#,
            )
            .bind(p.run_id)
            .bind(p.date)
            .bind(p.portfolio_value)
            .bind(p.cash)
            .bind(p.drawdown)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    async fn get_equity_curve(&self, run_id: Uuid) -> StorageResult<Vec<EquityCurvePoint>> {
        let rows = sqlx::query(
            r#"
            SELECT run_id, date, portfolio_value, cash, drawdown
            FROM backtest.equity_curve
            WHERE run_id = $1
            ORDER BY date ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| EquityCurvePoint {
                run_id: row.get("run_id"),
                date: row.get("date"),
                portfolio_value: row.get("portfolio_value"),
                cash: row.get("cash"),
                drawdown: row.get("drawdown"),
            })
            .collect())
    }
}

// ── Backtest row mappers ──────────────────────────────────────────────────────

fn backtest_run_from_pg(row: sqlx::postgres::PgRow) -> BacktestRunRow {
    BacktestRunRow {
        id: row.get("id"),
        config_id: row.get("config_id"),
        config_snapshot_json: row.get("config_snapshot_json"),
        from_date: row.get("from_date"),
        to_date: row.get("to_date"),
        initial_capital: row.get("initial_capital"),
        final_capital: row.get("final_capital"),
        cagr: row.get("cagr"),
        sharpe_ratio: row.get("sharpe_ratio"),
        sortino_ratio: row.get("sortino_ratio"),
        max_drawdown: row.get("max_drawdown"),
        max_drawdown_days: row.get("max_drawdown_days"),
        win_rate: row.get("win_rate"),
        profit_factor: row.get("profit_factor"),
        expectancy: row.get("expectancy"),
        total_trades: row.get("total_trades"),
        avg_hold_days: row.get("avg_hold_days"),
        status: row.get("status"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        error_message: row.get("error_message"),
    }
}

fn backtest_trade_from_pg(row: sqlx::postgres::PgRow) -> BacktestTradeRow {
    BacktestTradeRow {
        id: row.get("id"),
        run_id: row.get("run_id"),
        symbol: row.get("symbol"),
        direction: row.get("direction"),
        entry_date: row.get("entry_date"),
        entry_price: row.get("entry_price"),
        exit_date: row.get("exit_date"),
        exit_price: row.get("exit_price"),
        shares: row.get("shares"),
        gross_pnl: row.get("gross_pnl"),
        commission: row.get("commission"),
        net_pnl: row.get("net_pnl"),
        hold_days: row.get("hold_days"),
    }
}
