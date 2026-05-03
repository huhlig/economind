//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Performance metrics computation (§4.B).
//!
//! All metrics are derived from the closed trade log and the daily equity curve.
//! No database access — pure functions over in-memory data.

use crate::simulation::SimTrade;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use std::collections::BTreeMap;

// ── EquityPoint ───────────────────────────────────────────────────────────────

/// One day on the equity curve.
#[derive(Debug, Clone)]
pub struct EquityPoint {
    pub date: NaiveDate,
    pub value: Decimal,
    pub cash: Decimal,
    pub drawdown: Decimal,
}

// ── PerformanceMetrics ────────────────────────────────────────────────────────

/// All computed performance metrics for a completed backtest.
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    // ── Equity curve ──────────────────────────────────────────────────────────
    pub equity_curve: Vec<EquityPoint>,
    pub initial_capital: Decimal,
    pub final_capital: Decimal,

    // ── Return metrics ────────────────────────────────────────────────────────
    /// Compound annual growth rate (annualised total return).
    pub cagr: Decimal,
    /// Annualised Sharpe ratio (using risk-free rate from FRED if available, else 0).
    pub sharpe_ratio: Decimal,
    /// Sortino ratio (penalises downside volatility only).
    pub sortino_ratio: Decimal,

    // ── Drawdown ──────────────────────────────────────────────────────────────
    /// Maximum peak-to-trough drawdown as a fraction (0.0 – 1.0).
    pub max_drawdown: Decimal,
    /// Longest drawdown duration in calendar days.
    pub max_drawdown_days: i32,

    // ── Trade-level ───────────────────────────────────────────────────────────
    pub total_trades: i32,
    /// Fraction of closed trades that were profitable.
    pub win_rate: Decimal,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    /// gross_profit / gross_loss (> 1.0 is profitable).
    pub profit_factor: Decimal,
    /// Average expected P&L per trade (win_rate * avg_win − loss_rate * avg_loss).
    pub expectancy: Decimal,
    pub avg_hold_days: Decimal,
    pub largest_win: Decimal,
    pub largest_loss: Decimal,
}

impl PerformanceMetrics {
    /// Compute all metrics from the equity curve and trade log.
    ///
    /// `risk_free_daily` is the daily risk-free rate (e.g. DGS10 / 365).
    /// Pass `Decimal::ZERO` if not available.
    pub fn compute(
        daily_values: &BTreeMap<NaiveDate, (Decimal, Decimal)>, // date → (portfolio_value, cash)
        trades: &[SimTrade],
        initial_capital: Decimal,
        risk_free_daily: Decimal,
    ) -> Self {
        let equity_curve = build_equity_curve(daily_values);
        let final_capital = equity_curve
            .last()
            .map(|p| p.value)
            .unwrap_or(initial_capital);

        let cagr = compute_cagr(initial_capital, final_capital, daily_values.len() as u32);
        let daily_returns = compute_daily_returns(daily_values);
        let sharpe_ratio = compute_sharpe(&daily_returns, risk_free_daily);
        let sortino_ratio = compute_sortino(&daily_returns, risk_free_daily);
        let (max_drawdown, max_drawdown_days) = compute_max_drawdown(&equity_curve);

        let trade_metrics = compute_trade_metrics(trades);

        Self {
            equity_curve,
            initial_capital,
            final_capital,
            cagr,
            sharpe_ratio,
            sortino_ratio,
            max_drawdown,
            max_drawdown_days,
            total_trades: trade_metrics.total_trades,
            win_rate: trade_metrics.win_rate,
            avg_win: trade_metrics.avg_win,
            avg_loss: trade_metrics.avg_loss,
            profit_factor: trade_metrics.profit_factor,
            expectancy: trade_metrics.expectancy,
            avg_hold_days: trade_metrics.avg_hold_days,
            largest_win: trade_metrics.largest_win,
            largest_loss: trade_metrics.largest_loss,
        }
    }
}

// ── Equity curve builder ──────────────────────────────────────────────────────

fn build_equity_curve(
    daily_values: &BTreeMap<NaiveDate, (Decimal, Decimal)>,
) -> Vec<EquityPoint> {
    let mut peak = Decimal::ZERO;
    daily_values
        .iter()
        .map(|(date, (value, cash))| {
            if *value > peak {
                peak = *value;
            }
            let drawdown = if peak.is_zero() {
                Decimal::ZERO
            } else {
                ((peak - value) / peak).max(Decimal::ZERO)
            };
            EquityPoint {
                date: *date,
                value: *value,
                cash: *cash,
                drawdown,
            }
        })
        .collect()
}

// ── CAGR ──────────────────────────────────────────────────────────────────────

fn compute_cagr(initial: Decimal, final_val: Decimal, trading_days: u32) -> Decimal {
    if initial.is_zero() || trading_days == 0 {
        return Decimal::ZERO;
    }
    let years = Decimal::from(trading_days) / Decimal::from(252u32);
    if years.is_zero() {
        return Decimal::ZERO;
    }
    // CAGR = (final / initial)^(1/years) - 1
    // Using f64 for the power function, then converting back.
    let ratio = (final_val / initial).to_f64().unwrap_or(1.0);
    let years_f = years.to_f64().unwrap_or(1.0);
    let cagr_f = ratio.powf(1.0 / years_f) - 1.0;
    Decimal::from_f64(cagr_f).unwrap_or(Decimal::ZERO)
}

// ── Daily returns ─────────────────────────────────────────────────────────────

fn compute_daily_returns(
    daily_values: &BTreeMap<NaiveDate, (Decimal, Decimal)>,
) -> Vec<f64> {
    let values: Vec<f64> = daily_values
        .values()
        .map(|(v, _)| v.to_f64().unwrap_or(0.0))
        .collect();

    values
        .windows(2)
        .map(|w| if w[0] != 0.0 { (w[1] - w[0]) / w[0] } else { 0.0 })
        .collect()
}

// ── Sharpe ratio ──────────────────────────────────────────────────────────────

fn compute_sharpe(daily_returns: &[f64], risk_free_daily: Decimal) -> Decimal {
    if daily_returns.len() < 2 {
        return Decimal::ZERO;
    }
    let rf = risk_free_daily.to_f64().unwrap_or(0.0);
    let excess: Vec<f64> = daily_returns.iter().map(|r| r - rf).collect();
    let mean = excess.iter().sum::<f64>() / excess.len() as f64;
    let variance = excess.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
        / (excess.len() - 1) as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return Decimal::ZERO;
    }
    // Annualise: multiply daily Sharpe by sqrt(252)
    let sharpe = (mean / std_dev) * 252f64.sqrt();
    Decimal::from_f64(sharpe).unwrap_or(Decimal::ZERO)
}

// ── Sortino ratio ─────────────────────────────────────────────────────────────

fn compute_sortino(daily_returns: &[f64], risk_free_daily: Decimal) -> Decimal {
    if daily_returns.len() < 2 {
        return Decimal::ZERO;
    }
    let rf = risk_free_daily.to_f64().unwrap_or(0.0);
    let excess: Vec<f64> = daily_returns.iter().map(|r| r - rf).collect();
    let mean = excess.iter().sum::<f64>() / excess.len() as f64;
    // Downside deviation: only negative excess returns count.
    let downside_sq: Vec<f64> = excess.iter().map(|r| r.min(0.0).powi(2)).collect();
    let downside_var = downside_sq.iter().sum::<f64>() / (excess.len() - 1) as f64;
    let downside_std = downside_var.sqrt();
    if downside_std == 0.0 {
        return Decimal::ZERO;
    }
    let sortino = (mean / downside_std) * 252f64.sqrt();
    Decimal::from_f64(sortino).unwrap_or(Decimal::ZERO)
}

// ── Max drawdown ──────────────────────────────────────────────────────────────

fn compute_max_drawdown(equity_curve: &[EquityPoint]) -> (Decimal, i32) {
    let mut max_dd = Decimal::ZERO;
    let mut max_dd_days = 0i32;
    let mut drawdown_start: Option<NaiveDate> = None;

    for point in equity_curve {
        if point.drawdown > max_dd {
            max_dd = point.drawdown;
        }
        if point.drawdown > Decimal::ZERO {
            if drawdown_start.is_none() {
                drawdown_start = Some(point.date);
            }
        } else if let Some(start) = drawdown_start {
            let duration = (point.date - start).num_days() as i32;
            if duration > max_dd_days {
                max_dd_days = duration;
            }
            drawdown_start = None;
        }
    }
    // If still in drawdown at end of period
    if let (Some(start), Some(last)) = (drawdown_start, equity_curve.last()) {
        let duration = (last.date - start).num_days() as i32;
        if duration > max_dd_days {
            max_dd_days = duration;
        }
    }

    (max_dd, max_dd_days)
}

// ── Trade-level metrics ───────────────────────────────────────────────────────

struct TradeMetrics {
    total_trades: i32,
    win_rate: Decimal,
    avg_win: Decimal,
    avg_loss: Decimal,
    profit_factor: Decimal,
    expectancy: Decimal,
    avg_hold_days: Decimal,
    largest_win: Decimal,
    largest_loss: Decimal,
}

fn compute_trade_metrics(trades: &[SimTrade]) -> TradeMetrics {
    let total = trades.len() as i32;
    if total == 0 {
        return TradeMetrics {
            total_trades: 0,
            win_rate: Decimal::ZERO,
            avg_win: Decimal::ZERO,
            avg_loss: Decimal::ZERO,
            profit_factor: Decimal::ZERO,
            expectancy: Decimal::ZERO,
            avg_hold_days: Decimal::ZERO,
            largest_win: Decimal::ZERO,
            largest_loss: Decimal::ZERO,
        };
    }

    let wins: Vec<&SimTrade> = trades.iter().filter(|t| t.net_pnl > Decimal::ZERO).collect();
    let losses: Vec<&SimTrade> = trades.iter().filter(|t| t.net_pnl <= Decimal::ZERO).collect();

    let win_count = wins.len();
    let loss_count = losses.len();

    let win_rate = Decimal::from(win_count) / Decimal::from(total as u32);

    let avg_win = if win_count > 0 {
        wins.iter().map(|t| t.net_pnl).sum::<Decimal>() / Decimal::from(win_count as u32)
    } else {
        Decimal::ZERO
    };

    let avg_loss = if loss_count > 0 {
        losses.iter().map(|t| t.net_pnl.abs()).sum::<Decimal>()
            / Decimal::from(loss_count as u32)
    } else {
        Decimal::ZERO
    };

    let gross_profit: Decimal = wins.iter().map(|t| t.net_pnl).sum();
    let gross_loss: Decimal = losses.iter().map(|t| t.net_pnl.abs()).sum();
    let profit_factor = if gross_loss.is_zero() {
        if gross_profit > Decimal::ZERO { Decimal::from(999u32) } else { Decimal::ZERO }
    } else {
        gross_profit / gross_loss
    };

    let loss_rate = Decimal::ONE - win_rate;
    let expectancy = win_rate * avg_win - loss_rate * avg_loss;

    let avg_hold_days = trades.iter().map(|t| Decimal::from(t.hold_days)).sum::<Decimal>()
        / Decimal::from(total as u32);

    let largest_win = wins.iter().map(|t| t.net_pnl).fold(Decimal::ZERO, Decimal::max);
    let largest_loss = losses
        .iter()
        .map(|t| t.net_pnl.abs())
        .fold(Decimal::ZERO, Decimal::max);

    TradeMetrics {
        total_trades: total,
        win_rate,
        avg_win,
        avg_loss,
        profit_factor,
        expectancy,
        avg_hold_days,
        largest_win,
        largest_loss,
    }
}
