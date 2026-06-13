//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! MCP server for Economind (§7.A).
//!
//! Exposes platform data and operations as MCP tools so Claude (or any MCP
//! client) can query signals, instruments, portfolio state, backtest results,
//! and trigger strategy runs.
//!
//! # Tools
//!
//! | Tool                    | Description                                             |
//! |-------------------------|---------------------------------------------------------|
//! | `get_signals`           | Query recent trade signals with optional filters        |
//! | `get_instrument`        | Symbol metadata and recent bars                         |
//! | `get_portfolio`         | Current positions and portfolio state                   |
//! | `get_backtest_summary`  | Key metrics for a backtest run                          |
//! | `trigger_strategy_run`  | Fire an on-demand strategy run                          |
//! | `query_bars`            | OHLCV bars for a symbol over a date range               |
//! | `get_macro_context`     | Latest FRED macro series values                         |
//! | `analyze_signal`        | LLM-generated brief for a trade signal                  |
//! | `analyze_instrument`    | LLM-generated brief for an instrument                   |
//!
//! # Usage
//!
//! ```ignore
//! economind_agentic::mcp::serve(store, llm_client, 8081).await?;
//! ```

use std::sync::Arc;

use anyhow::Result;
use chrono::NaiveDate;
use economind_db::{
    BacktestStorage, CandleStorage, DataStore, MacroStorage, MetadataStorage, PortfolioStorage,
    StrategyStorage,
};
use futures::StreamExt;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_router,
};
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::{error, info};
use uuid::Uuid;

use crate::llm::LlmClient;

// ── Parameter structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSignalsParams {
    /// Filter by symbol (e.g. "AAPL"). Optional.
    pub symbol: Option<String>,
    /// Filter by strategy config UUID. Optional.
    pub config_id: Option<String>,
    /// Return signals emitted on or after this date (YYYY-MM-DD). Optional.
    pub since: Option<String>,
    /// Maximum number of results (default 20, max 100).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetInstrumentParams {
    /// Ticker symbol (e.g. "MSFT").
    pub symbol: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetBacktestSummaryParams {
    /// Backtest run UUID. Use "latest" to fetch the most recent run.
    pub run_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TriggerStrategyRunParams {
    /// Strategy config UUID to run.
    pub config_id: String,
    /// Lookback days for bar data (default 365).
    pub lookback_days: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryBarsParams {
    /// Ticker symbol.
    pub symbol: String,
    /// Start date inclusive (YYYY-MM-DD).
    pub from: String,
    /// End date inclusive (YYYY-MM-DD).
    pub to: String,
    /// Maximum number of bars to return (default 252).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeSignalParams {
    /// Trade signal UUID.
    pub signal_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeInstrumentParams {
    /// Ticker symbol.
    pub symbol: String,
}

// ── Server struct ─────────────────────────────────────────────────────────────

/// Economind MCP server — wraps a `DataStore` and optional LLM client.
#[derive(Clone)]
pub struct EconomindMcpServer {
    store: Arc<DataStore>,
    llm: Option<Arc<dyn LlmClient>>,
    tool_router: ToolRouter<EconomindMcpServer>,
}

#[tool_router]
impl EconomindMcpServer {
    fn new(store: DataStore, llm: Option<Arc<dyn LlmClient>>) -> Self {
        Self {
            store: Arc::new(store),
            llm,
            tool_router: Self::tool_router(),
        }
    }

    // ── get_signals ───────────────────────────────────────────────────────────

    #[tool(description = "\
        Query recent trade signals emitted by the strategy engine. \
        Optionally filter by symbol, strategy config ID, or date. \
        Returns up to `limit` signals (default 20) as a JSON array.")]
    async fn get_signals(
        &self,
        Parameters(p): Parameters<GetSignalsParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = p.limit.unwrap_or(20).min(100);

        let symbol = p.symbol
            .as_deref()
            .map(economind_core::model::Symbol::new);

        let config_id = p
            .config_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok());

        let since = p
            .since
            .as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        let rows = self
            .store
            .query_strategy_signals(None, config_id, symbol.as_ref(), since, Some(limit))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = serde_json::json!(rows.iter().map(|r| serde_json::json!({
            "id":                r.id,
            "run_id":            r.run_id,
            "config_id":         r.config_id,
            "symbol":            r.symbol,
            "direction":         r.direction,
            "identifier_score":  r.identifier_score,
            "timing_score":      r.timing_score,
            "position_shares":   r.position_shares,
            "position_notional": r.position_notional,
            "portfolio_fraction":r.portfolio_fraction,
            "rationale":         r.rationale,
            "analysis_brief":    r.analysis_brief,
            "emitted_at":        r.emitted_at,
        })).collect::<Vec<_>>());

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── get_instrument ────────────────────────────────────────────────────────

    #[tool(description = "\
        Return metadata and the 20 most recent daily OHLCV bars for a ticker symbol. \
        Useful for assessing an instrument before a trade.")]
    async fn get_instrument(
        &self,
        Parameters(p): Parameters<GetInstrumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let sym = economind_core::model::Symbol::new(&p.symbol);

        let ticker = self
            .store
            .get_ticker(&sym)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Fetch last ~20 bars by scanning a 30-day window.
        let to   = chrono::Utc::now().date_naive();
        let from = to - chrono::Duration::days(30);
        let bars = collect_bars(&self.store, &sym, from, to, 20)
            .await
            .unwrap_or_default();

        let json = serde_json::json!({
            "symbol":  p.symbol,
            "ticker":  ticker.map(|t| serde_json::json!({
                "name":      t.name,
                "sector":    format!("{:?}", t.sector),
                "industry":  format!("{:?}", t.industry),
                "marketcap": t.marketcap,
                "exchange":  t.exchange.map(|e| e.as_str().to_string()),
                "active":    t.active,
            })),
            "recent_bars": bars.iter().map(|b| serde_json::json!({
                "date":   b.date,
                "open":   b.open,
                "high":   b.high,
                "low":    b.low,
                "close":  b.close,
                "volume": b.volume,
            })).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── get_portfolio ─────────────────────────────────────────────────────────

    #[tool(description = "\
        Return the current portfolio state: open positions, total portfolio value, \
        available cash, and current drawdown from peak.")]
    async fn get_portfolio(&self) -> Result<CallToolResult, McpError> {
        let state = self
            .store
            .load_portfolio_state()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = serde_json::json!({
            "portfolio_value":  state.portfolio_value,
            "available_cash":   state.available_cash,
            "current_drawdown": state.current_drawdown,
            "open_positions": state.open_positions.iter().map(|p| serde_json::json!({
                "id":          p.id,
                "symbol":      p.symbol,
                "shares":      p.shares,
                "entry_price": p.entry_price,
                "entry_at":    p.entry_at,
            })).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── get_backtest_summary ──────────────────────────────────────────────────

    #[tool(description = "\
        Return key performance metrics for a backtest run. \
        Pass run_id as a UUID string, or \"latest\" to fetch the most recent run. \
        Metrics include CAGR, Sharpe, Sortino, max drawdown, win rate, and trade count.")]
    async fn get_backtest_summary(
        &self,
        Parameters(p): Parameters<GetBacktestSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        let run = if p.run_id.eq_ignore_ascii_case("latest") {
            self.store
                .list_backtest_runs(None, Some(1))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .into_iter()
                .next()
                .ok_or_else(|| McpError::invalid_params("No backtest runs found", None))?
        } else {
            let id = Uuid::parse_str(&p.run_id)
                .map_err(|_| McpError::invalid_params("Invalid run_id UUID", None))?;
            self.store
                .get_backtest_run(id)
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .ok_or_else(|| McpError::invalid_params("Backtest run not found", None))?
        };

        let json = serde_json::json!({
            "run_id":            run.id,
            "config_id":         run.config_id,
            "from_date":         run.from_date,
            "to_date":           run.to_date,
            "initial_capital":   run.initial_capital,
            "final_capital":     run.final_capital,
            "status":            run.status,
            "started_at":        run.started_at,
            "completed_at":      run.completed_at,
            "cagr":              run.cagr,
            "sharpe_ratio":      run.sharpe_ratio,
            "sortino_ratio":     run.sortino_ratio,
            "max_drawdown":      run.max_drawdown,
            "max_drawdown_days": run.max_drawdown_days,
            "win_rate":          run.win_rate,
            "profit_factor":     run.profit_factor,
            "expectancy":        run.expectancy,
            "total_trades":      run.total_trades,
            "avg_hold_days":     run.avg_hold_days,
        });

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── trigger_strategy_run ──────────────────────────────────────────────────

    #[tool(description = "\
        Trigger an on-demand strategy run for a given config UUID. \
        Returns the new run ID immediately; the run executes asynchronously. \
        Poll get_signals or the REST API to check for results.")]
    async fn trigger_strategy_run(
        &self,
        Parameters(p): Parameters<TriggerStrategyRunParams>,
    ) -> Result<CallToolResult, McpError> {
        let config_id = Uuid::parse_str(&p.config_id)
            .map_err(|_| McpError::invalid_params("Invalid config_id UUID", None))?;

        let config_row = self
            .store
            .get_strategy_config(config_id)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .ok_or_else(|| McpError::invalid_params("Strategy config not found", None))?;

        let run_id = Uuid::new_v4();
        let store  = Arc::clone(&self.store);
        let lookback = p.lookback_days.unwrap_or(365);

        // Fire and forget — return the run ID immediately.
        tokio::spawn(async move {
            if let Err(e) = run_strategy_stub(store, config_row, run_id, lookback).await {
                error!(%run_id, "Background strategy run failed: {e}");
            }
        });

        let json = serde_json::json!({
            "run_id":    run_id,
            "config_id": config_id,
            "status":    "running",
            "message":   "Strategy run started. Note: MCP trigger uses a stub — \
                          use POST /api/v1/strategy/run for full plugin execution.",
        });

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── query_bars ────────────────────────────────────────────────────────────

    #[tool(description = "\
        Return OHLCV daily bar data for a symbol over a date range. \
        Dates are YYYY-MM-DD. Returns at most `limit` bars (default 252).")]
    async fn query_bars(
        &self,
        Parameters(p): Parameters<QueryBarsParams>,
    ) -> Result<CallToolResult, McpError> {
        let sym  = economind_core::model::Symbol::new(&p.symbol);
        let from = NaiveDate::parse_from_str(&p.from, "%Y-%m-%d")
            .map_err(|_| McpError::invalid_params("Invalid from date (use YYYY-MM-DD)", None))?;
        let to   = NaiveDate::parse_from_str(&p.to, "%Y-%m-%d")
            .map_err(|_| McpError::invalid_params("Invalid to date (use YYYY-MM-DD)", None))?;
        let limit = p.limit.unwrap_or(252);

        let bars = collect_bars(&self.store, &sym, from, to, limit)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let json = serde_json::json!({
            "symbol": p.symbol,
            "from":   p.from,
            "to":     p.to,
            "count":  bars.len(),
            "bars": bars.iter().map(|b| serde_json::json!({
                "date":   b.date,
                "open":   b.open,
                "high":   b.high,
                "low":    b.low,
                "close":  b.close,
                "volume": b.volume,
            })).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(json.to_string())]))
    }

    // ── get_macro_context ─────────────────────────────────────────────────────

    #[tool(description = "\
        Return the latest values of all tracked FRED macro series: \
        10Y treasury yield, yield curve spread, CPI, unemployment rate, \
        VIX, and M2 money supply. Useful for injecting macro context into analysis.")]
    async fn get_macro_context(&self) -> Result<CallToolResult, McpError> {
        const SERIES: &[(&str, &str)] = &[
            ("DGS10",    "10-Year Treasury Yield (%)"),
            ("T10Y2Y",   "10Y-2Y Yield Curve Spread (%)"),
            ("CPIAUCSL", "CPI (YoY % change proxy)"),
            ("UNRATE",   "Unemployment Rate (%)"),
            ("VIXCLS",   "VIX Volatility Index"),
            ("M2SL",     "M2 Money Supply (billions USD)"),
        ];

        let ids: Vec<&str> = SERIES.iter().map(|(id, _)| *id).collect();
        let points = self
            .store
            .get_latest_macro_values(&ids)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let entries: Vec<serde_json::Value> = SERIES
            .iter()
            .filter_map(|(id, label)| {
                points.iter().find(|p| &p.series_id == id).map(|p| {
                    serde_json::json!({
                        "series_id": id,
                        "label":     label,
                        "date":      p.date,
                        "value":     p.value,
                    })
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "macro_context": entries }).to_string(),
        )]))
    }

    // ── analyze_signal ────────────────────────────────────────────────────────

    #[tool(description = "\
        Generate an LLM-powered natural-language brief for a trade signal. \
        Requires ANTHROPIC_API_KEY or LOCAL_LLM_BASE_URL to be set. \
        Returns an error message if no LLM backend is available.")]
    async fn analyze_signal(
        &self,
        Parameters(p): Parameters<AnalyzeSignalParams>,
    ) -> Result<CallToolResult, McpError> {
        let signal_id = Uuid::parse_str(&p.signal_id)
            .map_err(|_| McpError::invalid_params("Invalid signal_id UUID", None))?;

        let brief = crate::analysis::analyze_signal(
            &self.store,
            self.llm.as_deref(),
            signal_id,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .ok_or_else(|| {
            McpError::invalid_params(
                "No LLM backend configured. Set ANTHROPIC_API_KEY or LOCAL_LLM_BASE_URL.",
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(brief)]))
    }

    // ── analyze_instrument ────────────────────────────────────────────────────

    #[tool(description = "\
        Generate an LLM-powered natural-language brief for a ticker symbol. \
        Summarises price action and macro environment. \
        Requires ANTHROPIC_API_KEY or LOCAL_LLM_BASE_URL to be set.")]
    async fn analyze_instrument(
        &self,
        Parameters(p): Parameters<AnalyzeInstrumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let brief = crate::analysis::analyze_instrument(
            &self.store,
            self.llm.as_deref(),
            &p.symbol,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .ok_or_else(|| {
            McpError::invalid_params(
                "No LLM backend configured. Set ANTHROPIC_API_KEY or LOCAL_LLM_BASE_URL.",
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(brief)]))
    }
}

// ── ServerHandler impl ────────────────────────────────────────────────────────

impl ServerHandler for EconomindMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(
            Implementation::new("economind-mcp", env!("CARGO_PKG_VERSION")),
        )
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_instructions(
            "Economind MCP server — low-frequency trading analysis platform. \
             Available tools: get_signals, get_instrument, get_portfolio, \
             get_backtest_summary, trigger_strategy_run, query_bars, \
             get_macro_context, analyze_signal, analyze_instrument."
                .to_string(),
        )
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        self.tool_router.call_tool(self, request, context)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async { Ok(self.tool_router.list_tools()) }
    }
}

// ── Public server entry point ─────────────────────────────────────────────────

/// Start the Economind MCP server on `port`.
///
/// Accepts TCP connections and serves each on a dedicated task.
/// For a single-user platform this is typically one long-lived session at a time.
pub async fn serve(
    store: DataStore,
    llm: Option<Arc<dyn LlmClient>>,
    port: u16,
) -> Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("Economind MCP server listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        info!(%peer, "MCP client connected");

        let server = EconomindMcpServer::new(store.clone(), llm.clone());

        tokio::spawn(async move {
            match server.serve(stream).await {
                Ok(service) => {
                    if let Err(e) = service.waiting().await {
                        info!(%peer, "MCP session ended: {e}");
                    }
                }
                Err(e) => error!(%peer, "MCP serve error: {e}"),
            }
        });
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect up to `limit` daily bars from the streaming DataStore API.
pub(crate) async fn collect_bars(
    store: &DataStore,
    sym: &economind_core::model::Symbol,
    from: NaiveDate,
    to: NaiveDate,
    limit: usize,
) -> anyhow::Result<Vec<economind_core::model::DailyCandleEntry>> {
    let mut stream = store.query_daily_candles(sym, from..to).await?;
    let mut bars = Vec::new();
    while let Some(bar) = stream.next().await {
        bars.push(bar);
        if bars.len() >= limit {
            break;
        }
    }
    Ok(bars)
}

// ── Background strategy run stub ──────────────────────────────────────────────

async fn run_strategy_stub(
    store: Arc<DataStore>,
    config_row: economind_db::StrategyConfigRow,
    run_id: Uuid,
    _lookback_days: u32,
) -> anyhow::Result<()> {
    use economind_db::StrategyRunRow;

    let started_at = chrono::Utc::now();

    store
        .insert_strategy_run(&StrategyRunRow {
            id: run_id,
            config_id: config_row.id,
            started_at,
            completed_at: None,
            status: "running".to_string(),
            signal_count: 0,
            error_message: None,
            config_snapshot_json: config_row.parameters_json.clone(),
        })
        .await?;

    // MCP trigger_strategy_run is intentionally a stub — the full pipeline
    // runner requires strategy plugins which are statically compiled into
    // economind-cli and economind-api. Direct callers should use
    // POST /api/v1/strategy/run or `economind run --config <uuid>`.
    let msg = "MCP trigger_strategy_run stub: use POST /api/v1/strategy/run \
               or `economind run --config <uuid>` for full plugin execution.";

    store
        .complete_strategy_run(&StrategyRunRow {
            id: run_id,
            config_id: config_row.id,
            started_at,
            completed_at: Some(chrono::Utc::now()),
            status: "failed".to_string(),
            signal_count: 0,
            error_message: Some(msg.to_string()),
            config_snapshot_json: config_row.parameters_json,
        })
        .await?;

    Ok(())
}
