//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! GraphQL Query root (§5.C.2).

use async_graphql::{Context, Object, Result, ID};
use economind_core::model::Symbol;
use economind_db::{BacktestStorage, MetadataStorage, PortfolioStorage, StrategyStorage};
use futures::StreamExt;
use uuid::Uuid;

use crate::state::AppState;
use super::types::*;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    // ── Instruments ───────────────────────────────────────────────────────────

    /// List all tracked instruments.
    async fn instruments(&self, ctx: &Context<'_>) -> Result<Vec<GqlInstrument>> {
        let state = ctx.data::<AppState>()?;
        let mut stream = state.store().list_tickers().await?;
        let mut instruments = Vec::new();
        while let Some(sym) = stream.next().await {
            instruments.push(GqlInstrument {
                symbol: sym.as_str().to_string(),
                name: None,
                exchange: None,
                sector: None,
            });
        }
        Ok(instruments)
    }

    /// Fetch a single instrument by symbol.
    async fn instrument(
        &self,
        ctx: &Context<'_>,
        symbol: String,
    ) -> Result<Option<GqlInstrument>> {
        let state = ctx.data::<AppState>()?;
        let sym = Symbol::new(&symbol);
        let ticker = state.store().get_ticker(&sym).await?;
        Ok(ticker.map(|t| GqlInstrument {
            symbol: t.symbol.as_str().to_string(),
            name: t.name,
            exchange: t.exchange.as_ref().map(|e| e.as_str().to_string()),
            sector: t.sector.as_ref().map(|s| format!("{s:?}")),
        }))
    }

    // ── Signals ───────────────────────────────────────────────────────────────

    /// Query recent signals.  Optional filters: `config_id`, `symbol`, `limit`.
    async fn signals(
        &self,
        ctx: &Context<'_>,
        config_id: Option<ID>,
        symbol: Option<String>,
        limit: Option<i32>,
    ) -> Result<Vec<GqlSignal>> {
        let state = ctx.data::<AppState>()?;
        let cid = config_id
            .as_deref()
            .map(|s| Uuid::parse_str(s))
            .transpose()
            .map_err(|e| async_graphql::Error::new(format!("invalid config_id: {e}")))?;
        let sym = symbol.as_deref().map(Symbol::new);
        let rows = state
            .store()
            .query_strategy_signals(None, cid, sym.as_ref(), None, limit.map(|l| l as u32))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| GqlSignal {
                id: r.id.to_string().into(),
                run_id: r.run_id.to_string().into(),
                config_id: r.config_id.to_string().into(),
                symbol: r.symbol,
                direction: r.direction,
                identifier_score: r.identifier_score.to_string(),
                timing_score: r.timing_score.to_string(),
                position_shares: r.position_shares.map(|d| d.to_string()),
                position_notional: r.position_notional.map(|d| d.to_string()),
                portfolio_fraction: r.portfolio_fraction.map(|d| d.to_string()),
                rationale: r.rationale,
                analysis_brief: r.analysis_brief,
                emitted_at: r.emitted_at.to_rfc3339(),
            })
            .collect())
    }

    // ── Portfolio ─────────────────────────────────────────────────────────────

    /// Current portfolio state: open positions and cash balances.
    async fn portfolio(&self, ctx: &Context<'_>) -> Result<GqlPortfolio> {
        let state = ctx.data::<AppState>()?;
        let p = state.store().load_portfolio_state().await?;
        Ok(GqlPortfolio {
            portfolio_value: p.portfolio_value.to_string(),
            available_cash: p.available_cash.to_string(),
            current_drawdown: p.current_drawdown.to_string(),
            open_positions: p
                .open_positions
                .iter()
                .map(|pos| GqlPosition {
                    id: pos.id.to_string().into(),
                    symbol: pos.symbol.as_str().to_string(),
                    shares: pos.shares.to_string(),
                    entry_price: pos.entry_price.to_string(),
                    entry_at: pos.entry_at.to_rfc3339(),
                })
                .collect(),
        })
    }

    // ── Strategy configs ──────────────────────────────────────────────────────

    /// List all strategy configurations.
    async fn strategy_configs(&self, ctx: &Context<'_>) -> Result<Vec<GqlStrategyConfig>> {
        let state = ctx.data::<AppState>()?;
        let rows = state.store().list_strategy_configs().await?;
        Ok(rows
            .into_iter()
            .map(|r| GqlStrategyConfig {
                id: r.id.to_string().into(),
                name: r.name,
                description: r.description,
                composition: r.composition,
                plugins: r.plugins_json,
                parameters: r.parameters_json,
                enabled: r.enabled,
                version: r.version as i32,
                created_at: r.created_at.to_rfc3339(),
                updated_at: r.updated_at.to_rfc3339(),
            })
            .collect())
    }

    /// Fetch a single strategy configuration by ID.
    async fn strategy_config(
        &self,
        ctx: &Context<'_>,
        id: ID,
    ) -> Result<Option<GqlStrategyConfig>> {
        let state = ctx.data::<AppState>()?;
        let uid = Uuid::parse_str(&id)
            .map_err(|e| async_graphql::Error::new(format!("invalid id: {e}")))?;
        let row = state.store().get_strategy_config(uid).await?;
        Ok(row.map(|r| GqlStrategyConfig {
            id: r.id.to_string().into(),
            name: r.name,
            description: r.description,
            composition: r.composition,
            plugins: r.plugins_json,
            parameters: r.parameters_json,
            enabled: r.enabled,
            version: r.version as i32,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }))
    }

    // ── Backtest ──────────────────────────────────────────────────────────────

    /// List backtest runs.
    async fn backtest_runs(
        &self,
        ctx: &Context<'_>,
        limit: Option<i32>,
    ) -> Result<Vec<GqlBacktestRun>> {
        let state = ctx.data::<AppState>()?;
        let rows = state
            .store()
            .list_backtest_runs(None, limit.map(|l| l as u32))
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| GqlBacktestRun {
                id: r.id.to_string().into(),
                config_id: r.config_id.to_string().into(),
                status: r.status,
                from_date: r.from_date.to_string(),
                to_date: r.to_date.to_string(),
                initial_capital: Some(r.initial_capital.to_string()),
                final_capital: r.final_capital.map(|d| d.to_string()),
                cagr: r.cagr.map(|d| d.to_string()),
                sharpe_ratio: r.sharpe_ratio.map(|d| d.to_string()),
                sortino_ratio: r.sortino_ratio.map(|d| d.to_string()),
                max_drawdown: r.max_drawdown.map(|d| d.to_string()),
                max_drawdown_days: r.max_drawdown_days,
                total_trades: r.total_trades,
                win_rate: r.win_rate.map(|d| d.to_string()),
                profit_factor: r.profit_factor.map(|d| d.to_string()),
                expectancy: r.expectancy.map(|d| d.to_string()),
                started_at: r.started_at.to_rfc3339(),
                completed_at: r.completed_at.map(|d| d.to_rfc3339()),
            })
            .collect())
    }

    /// Fetch a single backtest run by ID.
    async fn backtest_run(&self, ctx: &Context<'_>, id: ID) -> Result<Option<GqlBacktestRun>> {
        let state = ctx.data::<AppState>()?;
        let uid = Uuid::parse_str(&id)
            .map_err(|e| async_graphql::Error::new(format!("invalid id: {e}")))?;
        let row = state.store().get_backtest_run(uid).await?;
        Ok(row.map(|r| GqlBacktestRun {
            id: r.id.to_string().into(),
            config_id: r.config_id.to_string().into(),
            status: r.status,
            from_date: r.from_date.to_string(),
            to_date: r.to_date.to_string(),
            initial_capital: Some(r.initial_capital.to_string()),
            final_capital: r.final_capital.map(|d| d.to_string()),
            cagr: r.cagr.map(|d| d.to_string()),
            sharpe_ratio: r.sharpe_ratio.map(|d| d.to_string()),
            sortino_ratio: r.sortino_ratio.map(|d| d.to_string()),
            max_drawdown: r.max_drawdown.map(|d| d.to_string()),
            max_drawdown_days: r.max_drawdown_days,
            total_trades: r.total_trades,
            win_rate: r.win_rate.map(|d| d.to_string()),
            profit_factor: r.profit_factor.map(|d| d.to_string()),
            expectancy: r.expectancy.map(|d| d.to_string()),
            started_at: r.started_at.to_rfc3339(),
            completed_at: r.completed_at.map(|d| d.to_rfc3339()),
        }))
    }
}
