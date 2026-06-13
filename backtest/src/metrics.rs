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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::SimTrade;
    use chrono::NaiveDate;
    use economind_core::model::Symbol;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromStr;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn d(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }

    fn nil_uuid() -> Uuid { Uuid::nil() }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Build a BTreeMap of daily equity curve values.
    /// `values` is a slice of (year, month, day, portfolio_value_f64, cash_f64).
    fn equity_map(values: &[(i32, u32, u32, f64, f64)]) -> BTreeMap<NaiveDate, (Decimal, Decimal)> {
        values.iter().map(|&(y, m, d, v, c)| {
            (date(y, m, d), (Decimal::from_f64(v).unwrap(), Decimal::from_f64(c).unwrap()))
        }).collect()
    }

    fn make_trade(net_pnl: f64, hold_days: i32) -> SimTrade {
        SimTrade {
            id: nil_uuid(),
            symbol: Symbol::new("TEST"),
            direction: "long".to_string(),
            entry_date: date(2024, 1, 1),
            entry_price: d("100"),
            exit_date: date(2024, 1, 1 + hold_days as u32),
            exit_price: d("110"),
            shares: d("10"),
            gross_pnl: Decimal::from_f64(net_pnl + 2.0).unwrap(),
            commission: d("2"),
            net_pnl: Decimal::from_f64(net_pnl).unwrap(),
            hold_days,
        }
    }

    // ── build_equity_curve ────────────────────────────────────────────────────

    #[test]
    fn equity_curve_empty_input() {
        let map: BTreeMap<NaiveDate, (Decimal, Decimal)> = BTreeMap::new();
        let curve = build_equity_curve(&map);
        assert!(curve.is_empty());
    }

    #[test]
    fn equity_curve_monotone_rising_has_zero_drawdown() {
        let map = equity_map(&[
            (2024, 1, 1, 100_000.0, 100_000.0),
            (2024, 1, 2, 101_000.0, 50_000.0),
            (2024, 1, 3, 102_000.0, 40_000.0),
        ]);
        let curve = build_equity_curve(&map);
        assert_eq!(curve.len(), 3);
        for pt in &curve {
            assert_eq!(pt.drawdown, Decimal::ZERO, "rising series should have 0 drawdown");
        }
    }

    #[test]
    fn equity_curve_drawdown_computed_correctly() {
        // Peak at 110_000, then drops to 99_000 → drawdown = 10_000/110_000 ≈ 9.09%
        let map = equity_map(&[
            (2024, 1, 1, 100_000.0, 100_000.0),
            (2024, 1, 2, 110_000.0,  50_000.0),
            (2024, 1, 3,  99_000.0,  30_000.0),
        ]);
        let curve = build_equity_curve(&map);
        let dd = curve[2].drawdown.to_f64().unwrap();
        assert!((dd - 10_000.0 / 110_000.0).abs() < 1e-6, "drawdown={:.6}", dd);
    }

    // ── compute_cagr ─────────────────────────────────────────────────────────

    #[test]
    fn cagr_zero_initial_returns_zero() {
        assert_eq!(compute_cagr(Decimal::ZERO, d("110000"), 252), Decimal::ZERO);
    }

    #[test]
    fn cagr_zero_trading_days_returns_zero() {
        assert_eq!(compute_cagr(d("100000"), d("110000"), 0), Decimal::ZERO);
    }

    #[test]
    fn cagr_no_growth_returns_zero() {
        // 100_000 → 100_000 over 252 days → CAGR = 0%
        let cagr = compute_cagr(d("100000"), d("100000"), 252).to_f64().unwrap();
        assert!(cagr.abs() < 1e-6);
    }

    #[test]
    fn cagr_doubles_in_one_year_is_100pct() {
        // 100_000 → 200_000 in 252 trading days → CAGR ≈ 100%
        let cagr = compute_cagr(d("100000"), d("200000"), 252).to_f64().unwrap();
        assert!((cagr - 1.0).abs() < 1e-4, "cagr={:.6}", cagr);
    }

    #[test]
    fn cagr_positive_for_growing_portfolio() {
        let cagr = compute_cagr(d("100000"), d("120000"), 252).to_f64().unwrap();
        assert!(cagr > 0.0);
    }

    // ── compute_sharpe / compute_sortino ──────────────────────────────────────

    #[test]
    fn sharpe_insufficient_returns_zero() {
        assert_eq!(compute_sharpe(&[], Decimal::ZERO), Decimal::ZERO);
        assert_eq!(compute_sharpe(&[0.01], Decimal::ZERO), Decimal::ZERO);
    }

    #[test]
    fn sharpe_positive_for_consistently_rising_returns() {
        // All returns 0.1% daily — positive mean, near-zero variance → high Sharpe
        let returns: Vec<f64> = vec![0.001; 252];
        let sharpe = compute_sharpe(&returns, Decimal::ZERO).to_f64().unwrap();
        assert!(sharpe > 1.0, "sharpe={}", sharpe);
    }

    #[test]
    fn sharpe_zero_for_constant_zero_returns() {
        let returns: Vec<f64> = vec![0.0; 252];
        let sharpe = compute_sharpe(&returns, Decimal::ZERO);
        // std_dev is 0 → should return ZERO
        assert_eq!(sharpe, Decimal::ZERO);
    }

    #[test]
    fn sortino_insufficient_returns_zero() {
        assert_eq!(compute_sortino(&[], Decimal::ZERO), Decimal::ZERO);
    }

    #[test]
    fn sortino_positive_for_upward_trend_no_downside() {
        // Only positive returns → downside std_dev = 0 → returns ZERO (no downside to penalise)
        let returns: Vec<f64> = vec![0.002; 252];
        let sortino = compute_sortino(&returns, Decimal::ZERO);
        // All returns positive → no downside volatility → formula returns ZERO
        assert_eq!(sortino, Decimal::ZERO);
    }

    #[test]
    fn sortino_positive_when_mean_exceeds_downside_vol() {
        // Mix of moderate ups and small downs → positive Sortino
        let mut returns = vec![0.005f64; 200];
        returns.extend(vec![-0.001f64; 52]);
        let sortino = compute_sortino(&returns, Decimal::ZERO).to_f64().unwrap();
        assert!(sortino > 0.0, "sortino={}", sortino);
    }

    // ── compute_max_drawdown ─────────────────────────────────────────────────

    #[test]
    fn max_drawdown_empty_returns_zero() {
        let (dd, days) = compute_max_drawdown(&[]);
        assert_eq!(dd, Decimal::ZERO);
        assert_eq!(days, 0);
    }

    #[test]
    fn max_drawdown_monotone_rising_is_zero() {
        let map = equity_map(&[
            (2024, 1, 1, 100_000.0, 100_000.0),
            (2024, 1, 2, 105_000.0,  80_000.0),
            (2024, 1, 3, 110_000.0,  60_000.0),
        ]);
        let curve = build_equity_curve(&map);
        let (dd, days) = compute_max_drawdown(&curve);
        assert_eq!(dd, Decimal::ZERO);
        assert_eq!(days, 0);
    }

    #[test]
    fn max_drawdown_peak_trough_computed() {
        // Peak 110k, trough 88k → max drawdown = 20%
        let map = equity_map(&[
            (2024, 1, 1, 100_000.0, 100_000.0),
            (2024, 1, 2, 110_000.0,  50_000.0),
            (2024, 1, 3,  88_000.0,  10_000.0),
            (2024, 1, 4, 110_000.0,  50_000.0), // recovery
        ]);
        let curve = build_equity_curve(&map);
        let (dd, _) = compute_max_drawdown(&curve);
        let dd_f = dd.to_f64().unwrap();
        assert!((dd_f - 22_000.0 / 110_000.0).abs() < 1e-6, "dd={:.6}", dd_f);
    }

    // ── compute_trade_metrics ─────────────────────────────────────────────────

    #[test]
    fn trade_metrics_no_trades() {
        let m = compute_trade_metrics(&[]);
        assert_eq!(m.total_trades, 0);
        assert_eq!(m.win_rate, Decimal::ZERO);
        assert_eq!(m.profit_factor, Decimal::ZERO);
    }

    #[test]
    fn trade_metrics_all_wins() {
        let trades = vec![make_trade(100.0, 5), make_trade(50.0, 3)];
        let m = compute_trade_metrics(&trades);
        assert_eq!(m.total_trades, 2);
        assert_eq!(m.win_rate, Decimal::ONE);
        assert_eq!(m.avg_loss, Decimal::ZERO);
        // No losses → profit_factor is capped at 999
        assert_eq!(m.profit_factor, Decimal::from(999u32));
    }

    #[test]
    fn trade_metrics_all_losses() {
        let trades = vec![make_trade(-80.0, 4), make_trade(-40.0, 2)];
        let m = compute_trade_metrics(&trades);
        assert_eq!(m.total_trades, 2);
        assert_eq!(m.win_rate, Decimal::ZERO);
        assert_eq!(m.avg_win, Decimal::ZERO);
        assert_eq!(m.profit_factor, Decimal::ZERO);
    }

    #[test]
    fn trade_metrics_win_rate_50pct() {
        let trades = vec![make_trade(100.0, 5), make_trade(-50.0, 3)];
        let m = compute_trade_metrics(&trades);
        assert_eq!(m.total_trades, 2);
        let wr = m.win_rate.to_f64().unwrap();
        assert!((wr - 0.5).abs() < 1e-6, "win_rate={}", wr);
    }

    #[test]
    fn trade_metrics_profit_factor_two_to_one() {
        // gross_profit = 200, gross_loss = 100 → profit_factor = 2.0
        let trades = vec![make_trade(200.0, 5), make_trade(-100.0, 3)];
        let m = compute_trade_metrics(&trades);
        let pf = m.profit_factor.to_f64().unwrap();
        assert!((pf - 2.0).abs() < 1e-6, "profit_factor={}", pf);
    }

    #[test]
    fn trade_metrics_expectancy_positive_edge() {
        // win_rate=0.6, avg_win=100, avg_loss=50 → expectancy = 0.6*100 - 0.4*50 = 40
        let trades = vec![
            make_trade(100.0, 5), make_trade(100.0, 4), make_trade(100.0, 3),
            make_trade(-50.0, 2), make_trade(-50.0, 1),
        ];
        let m = compute_trade_metrics(&trades);
        let exp = m.expectancy.to_f64().unwrap();
        assert!(exp > 0.0, "expectancy should be positive with 3:2 win ratio, got {}", exp);
    }

    #[test]
    fn trade_metrics_hold_days_average() {
        let trades = vec![make_trade(10.0, 4), make_trade(10.0, 6)];
        let m = compute_trade_metrics(&trades);
        let avg = m.avg_hold_days.to_f64().unwrap();
        assert!((avg - 5.0).abs() < 1e-6, "avg_hold_days={}", avg);
    }

    #[test]
    fn trade_metrics_largest_win_and_loss() {
        let trades = vec![
            make_trade(200.0, 5), make_trade(50.0, 3),   // wins
            make_trade(-150.0, 4), make_trade(-30.0, 2), // losses
        ];
        let m = compute_trade_metrics(&trades);
        assert_eq!(m.largest_win, d("200"));
        assert_eq!(m.largest_loss, d("150"));
    }

    // ── PerformanceMetrics::compute (integration) ─────────────────────────────

    #[test]
    fn perf_metrics_compute_growing_portfolio() {
        let map = equity_map(&[
            (2024, 1,  1, 100_000.0, 100_000.0),
            (2024, 1,  2, 100_500.0,  80_000.0),
            (2024, 1,  3, 101_000.0,  60_000.0),
            (2024, 1,  4, 101_500.0,  40_000.0),
            (2024, 1,  5, 102_000.0,  20_000.0),
        ]);
        let trades = vec![make_trade(2_000.0, 4)];
        let m = PerformanceMetrics::compute(&map, &trades, d("100000"), Decimal::ZERO);

        assert!(m.final_capital > m.initial_capital);
        assert!(m.cagr > Decimal::ZERO);
        assert!(m.sharpe_ratio > Decimal::ZERO);
        assert_eq!(m.max_drawdown, Decimal::ZERO);
        assert_eq!(m.total_trades, 1);
        assert_eq!(m.win_rate, Decimal::ONE);
    }

    #[test]
    fn perf_metrics_compute_empty_trades() {
        let map = equity_map(&[
            (2024, 1, 1, 100_000.0, 100_000.0),
            (2024, 1, 2, 100_000.0, 100_000.0),
        ]);
        let m = PerformanceMetrics::compute(&map, &[], d("100000"), Decimal::ZERO);
        assert_eq!(m.total_trades, 0);
        assert_eq!(m.win_rate, Decimal::ZERO);
    }
}
