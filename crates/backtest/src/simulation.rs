//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Simulated portfolio state and order execution (§4.A.3, §4.A.4).
//!
//! Tracks cash, open positions, and closed trades over a backtest period.
//! Orders fill at next-day open with configurable slippage and commission.

use chrono::NaiveDate;
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

// ── SimTrade ──────────────────────────────────────────────────────────────────

/// A completed simulated trade (both entry and exit recorded).
#[derive(Debug, Clone)]
pub struct SimTrade {
    pub id: Uuid,
    pub symbol: Symbol,
    /// `"long"` or `"short"`.
    pub direction: String,
    pub entry_date: NaiveDate,
    pub entry_price: Decimal,
    pub exit_date: NaiveDate,
    pub exit_price: Decimal,
    pub shares: Decimal,
    pub gross_pnl: Decimal,
    pub commission: Decimal,
    pub net_pnl: Decimal,
    pub hold_days: i32,
}

// ── OpenSimPosition ───────────────────────────────────────────────────────────

/// An open simulated position — not yet exited.
#[derive(Debug, Clone)]
pub struct OpenSimPosition {
    pub id: Uuid,
    pub symbol: Symbol,
    pub direction: String,
    pub entry_date: NaiveDate,
    pub entry_price: Decimal,
    pub shares: Decimal,
    pub commission_paid: Decimal,
}

// ── SimPortfolio ──────────────────────────────────────────────────────────────

/// In-memory portfolio state used during a backtest simulation.
///
/// One `SimPortfolio` lives for the entire backtest run; `step()` is called
/// once per trading day. At the end, `closed_trades` holds the full trade log.
#[derive(Debug)]
pub struct SimPortfolio {
    /// Available cash.
    pub cash: Decimal,
    /// Currently open positions keyed by symbol.
    pub open_positions: HashMap<Symbol, OpenSimPosition>,
    /// All closed trades (completed round-trips).
    pub closed_trades: Vec<SimTrade>,
    /// Commission charged per trade entry or exit.
    pub commission_per_trade: Decimal,
    /// Slippage in basis points applied to fill prices (both entry and exit).
    pub slippage_bps: u32,
    /// Maximum fraction of portfolio that any single position may represent.
    pub max_position_pct: Decimal,
    /// Peak portfolio value seen so far (for drawdown tracking).
    peak_value: Decimal,
}

impl SimPortfolio {
    pub fn new(
        initial_capital: Decimal,
        commission_per_trade: Decimal,
        slippage_bps: u32,
        max_position_pct: Decimal,
    ) -> Self {
        Self {
            cash: initial_capital,
            open_positions: HashMap::new(),
            closed_trades: Vec::new(),
            commission_per_trade,
            slippage_bps,
            max_position_pct,
            peak_value: initial_capital,
        }
    }

    // ── Fill price helpers ────────────────────────────────────────────────────

    /// Apply slippage to a fill price.
    ///
    /// For entries: price is nudged up (buying at a slight premium).
    /// For exits:   price is nudged down (selling at a slight discount).
    fn apply_slippage(&self, price: Decimal, is_entry: bool) -> Decimal {
        let bps = Decimal::from(self.slippage_bps);
        let factor = bps / Decimal::from(10_000u32);
        if is_entry {
            price * (Decimal::ONE + factor)
        } else {
            price * (Decimal::ONE - factor)
        }
    }

    // ── Entry ─────────────────────────────────────────────────────────────────

    /// Attempt to open a new long position for `symbol` on `date`.
    ///
    /// `open_price` is the next-day open (already determined by the caller).
    /// `target_shares` is the computed position size from the Sizer.
    ///
    /// Returns `true` if the position was opened (enough cash available),
    /// `false` if it was rejected due to insufficient funds or an existing
    /// position in that symbol.
    pub fn enter_long(
        &mut self,
        symbol: Symbol,
        date: NaiveDate,
        open_price: Decimal,
        target_shares: Decimal,
    ) -> bool {
        if self.open_positions.contains_key(&symbol) {
            return false; // already in a position for this symbol
        }
        let fill_price = self.apply_slippage(open_price, true);
        let cost = fill_price * target_shares + self.commission_per_trade;
        if cost > self.cash {
            return false;
        }
        self.cash -= cost;
        self.open_positions.insert(
            symbol.clone(),
            OpenSimPosition {
                id: Uuid::new_v4(),
                symbol,
                direction: "long".to_string(),
                entry_date: date,
                entry_price: fill_price,
                shares: target_shares,
                commission_paid: self.commission_per_trade,
            },
        );
        true
    }

    // ── Exit ──────────────────────────────────────────────────────────────────

    /// Close an open position for `symbol` on `date` at `open_price`.
    ///
    /// Returns the completed `SimTrade` if a position existed, `None` otherwise.
    pub fn exit_position(
        &mut self,
        symbol: &Symbol,
        date: NaiveDate,
        open_price: Decimal,
    ) -> Option<SimTrade> {
        let pos = self.open_positions.remove(symbol)?;
        let fill_price = self.apply_slippage(open_price, false);
        let proceeds = fill_price * pos.shares;
        let exit_commission = self.commission_per_trade;
        self.cash += proceeds - exit_commission;

        let gross_pnl = (fill_price - pos.entry_price) * pos.shares;
        let total_commission = pos.commission_paid + exit_commission;
        let net_pnl = gross_pnl - total_commission;
        let hold_days = (date - pos.entry_date).num_days() as i32;

        let trade = SimTrade {
            id: pos.id,
            symbol: pos.symbol,
            direction: pos.direction,
            entry_date: pos.entry_date,
            entry_price: pos.entry_price,
            exit_date: date,
            exit_price: fill_price,
            shares: pos.shares,
            gross_pnl,
            commission: total_commission,
            net_pnl,
            hold_days,
        };
        self.closed_trades.push(trade.clone());
        Some(trade)
    }

    // ── Mark-to-market ────────────────────────────────────────────────────────

    /// Compute the current total portfolio value given a map of latest prices.
    pub fn portfolio_value(&self, prices: &HashMap<Symbol, Decimal>) -> Decimal {
        let position_value: Decimal = self
            .open_positions
            .values()
            .map(|p| {
                let price = prices.get(&p.symbol).copied().unwrap_or(p.entry_price);
                price * p.shares
            })
            .sum();
        self.cash + position_value
    }

    /// Compute drawdown fraction from peak (0.0 = at peak, 1.0 = total loss).
    pub fn drawdown(&mut self, current_value: Decimal) -> Decimal {
        if current_value > self.peak_value {
            self.peak_value = current_value;
        }
        if self.peak_value.is_zero() {
            return Decimal::ZERO;
        }
        ((self.peak_value - current_value) / self.peak_value).max(Decimal::ZERO)
    }

    /// Close all remaining open positions at the given prices (end-of-backtest).
    pub fn close_all(&mut self, date: NaiveDate, prices: &HashMap<Symbol, Decimal>) {
        let symbols: Vec<Symbol> = self.open_positions.keys().cloned().collect();
        for sym in symbols {
            let price = prices
                .get(&sym)
                .copied()
                .unwrap_or_else(|| self.open_positions[&sym].entry_price);
            self.exit_position(&sym, date, price);
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal::prelude::FromStr;
    use rust_decimal::Decimal;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn portfolio() -> SimPortfolio {
        SimPortfolio::new(
            d("100000"), // initial capital
            d("1"),      // $1 commission per trade
            5,           // 5 bps slippage
            d("0.25"),   // 25% max position size
        )
    }

    // ── Construction ─────────────────────────────────────────────────────────

    #[test]
    fn new_portfolio_has_correct_cash() {
        let p = portfolio();
        assert_eq!(p.cash, d("100000"));
        assert!(p.open_positions.is_empty());
        assert!(p.closed_trades.is_empty());
    }

    // ── apply_slippage ────────────────────────────────────────────────────────

    #[test]
    fn slippage_increases_entry_price() {
        let p = portfolio();
        let price = d("100");
        let entry_fill = p.apply_slippage(price, true);
        assert!(entry_fill > price, "entry fill should be above quote price");
    }

    #[test]
    fn slippage_decreases_exit_price() {
        let p = portfolio();
        let price = d("100");
        let exit_fill = p.apply_slippage(price, false);
        assert!(exit_fill < price, "exit fill should be below quote price");
    }

    #[test]
    fn slippage_5bps_on_100_is_correct() {
        let p = portfolio();
        let entry = p.apply_slippage(d("100"), true);
        // 5 bps = 0.05% → 100 * 1.0005 = 100.05
        assert_eq!(entry, d("100.05"));
        let exit_p = p.apply_slippage(d("100"), false);
        assert_eq!(exit_p, d("99.95"));
    }

    // ── enter_long ────────────────────────────────────────────────────────────

    #[test]
    fn enter_long_success_reduces_cash() {
        let mut p = portfolio();
        let sym = Symbol::new("AAPL");
        let result = p.enter_long(sym.clone(), date(2024, 1, 2), d("100"), d("10"));
        assert!(result, "should succeed with sufficient cash");
        // cost = 100.05 * 10 + 1 = 1001.50
        assert!(p.cash < d("100000"), "cash should be reduced after entry");
        assert!(p.open_positions.contains_key(&sym));
    }

    #[test]
    fn enter_long_rejects_duplicate_symbol() {
        let mut p = portfolio();
        let sym = Symbol::new("AAPL");
        p.enter_long(sym.clone(), date(2024, 1, 2), d("100"), d("10"));
        let result = p.enter_long(sym.clone(), date(2024, 1, 3), d("102"), d("10"));
        assert!(!result, "duplicate entry should be rejected");
    }

    #[test]
    fn enter_long_rejects_insufficient_cash() {
        let mut p = portfolio();
        let sym = Symbol::new("AAPL");
        // Try to buy 2000 shares @ 100 = $200,000 — exceeds $100,000 capital
        let result = p.enter_long(sym, date(2024, 1, 2), d("100"), d("2000"));
        assert!(!result, "should reject when cost exceeds cash");
    }

    // ── exit_position ─────────────────────────────────────────────────────────

    #[test]
    fn exit_position_produces_correct_trade() {
        let mut p = portfolio();
        let sym = Symbol::new("MSFT");
        p.enter_long(sym.clone(), date(2024, 1, 2), d("200"), d("5"));
        // Exit at $220: 5 shares * (220 * 0.9995) - entry costs
        let trade = p.exit_position(&sym, date(2024, 1, 10), d("220"));
        assert!(trade.is_some(), "should return a trade");
        let t = trade.unwrap();
        assert_eq!(t.symbol, sym);
        assert_eq!(t.direction, "long");
        assert_eq!(t.hold_days, 8);
        assert!(
            t.gross_pnl > Decimal::ZERO,
            "gross_pnl should be positive (price up)"
        );
        assert!(
            t.net_pnl < t.gross_pnl,
            "net_pnl < gross_pnl after commission"
        );
    }

    #[test]
    fn exit_position_none_for_unknown_symbol() {
        let mut p = portfolio();
        let sym = Symbol::new("UNKNOWN");
        assert!(p.exit_position(&sym, date(2024, 1, 10), d("100")).is_none());
    }

    #[test]
    fn exit_position_adds_to_closed_trades() {
        let mut p = portfolio();
        let sym = Symbol::new("GOOG");
        p.enter_long(sym.clone(), date(2024, 1, 2), d("150"), d("10"));
        p.exit_position(&sym, date(2024, 1, 5), d("160"));
        assert_eq!(p.closed_trades.len(), 1);
        assert!(p.open_positions.is_empty());
    }

    #[test]
    fn exit_position_increases_cash_on_profit() {
        let mut p = portfolio();
        let sym = Symbol::new("TSLA");
        let cash_before = p.cash;
        p.enter_long(sym.clone(), date(2024, 1, 2), d("100"), d("10"));
        let cash_after_entry = p.cash;
        p.exit_position(&sym, date(2024, 1, 5), d("120")); // +20% gain
        let cash_after_exit = p.cash;
        assert!(
            cash_after_exit > cash_after_entry,
            "cash should increase after profitable exit"
        );
        assert!(
            cash_after_exit > cash_before - d("5"),
            "net of costs, cash should be near original"
        );
    }

    // ── portfolio_value ───────────────────────────────────────────────────────

    #[test]
    fn portfolio_value_all_cash_no_positions() {
        let p = portfolio();
        let prices: HashMap<Symbol, Decimal> = HashMap::new();
        assert_eq!(p.portfolio_value(&prices), d("100000"));
    }

    #[test]
    fn portfolio_value_includes_open_positions_at_mark() {
        let mut p = portfolio();
        let sym = Symbol::new("NVDA");
        p.enter_long(sym.clone(), date(2024, 1, 2), d("100"), d("10"));
        // Mark position at $120
        let mut prices = HashMap::new();
        prices.insert(sym, d("120"));
        let val = p.portfolio_value(&prices);
        // cash_after_entry + 10 * 120
        let expected_pos_value = d("120") * d("10");
        assert!(
            val > d("100000"),
            "portfolio value should exceed capital due to mark-up"
        );
        assert_eq!(val, p.cash + expected_pos_value);
    }

    #[test]
    fn portfolio_value_falls_back_to_entry_price_when_no_mark() {
        let mut p = portfolio();
        let sym = Symbol::new("AMD");
        p.enter_long(sym.clone(), date(2024, 1, 2), d("50"), d("20"));
        // No price provided → falls back to entry price
        let prices: HashMap<Symbol, Decimal> = HashMap::new();
        let val = p.portfolio_value(&prices);
        let pos = &p.open_positions[&sym];
        // entry_price includes slippage: 50 * 1.0005 = 50.025
        let expected = p.cash + pos.entry_price * pos.shares;
        assert_eq!(val, expected);
    }

    // ── drawdown ─────────────────────────────────────────────────────────────

    #[test]
    fn drawdown_starts_at_zero() {
        let mut p = portfolio();
        assert_eq!(p.drawdown(d("100000")), Decimal::ZERO);
    }

    #[test]
    fn drawdown_positive_when_below_peak() {
        let mut p = portfolio();
        p.drawdown(d("110000")); // new peak
        let dd = p.drawdown(d("99000"));
        assert!(dd > Decimal::ZERO, "should be in drawdown");
        let expected = (d("110000") - d("99000")) / d("110000");
        assert!((dd - expected).abs() < d("0.000001"));
    }

    #[test]
    fn drawdown_zero_at_new_peak() {
        let mut p = portfolio();
        p.drawdown(d("100000"));
        p.drawdown(d("90000")); // drawdown
        let dd = p.drawdown(d("110000")); // new peak
        assert_eq!(dd, Decimal::ZERO);
    }

    // ── close_all ─────────────────────────────────────────────────────────────

    #[test]
    fn close_all_no_open_positions_noop() {
        let mut p = portfolio();
        let prices: HashMap<Symbol, Decimal> = HashMap::new();
        p.close_all(date(2024, 1, 10), &prices);
        assert!(p.closed_trades.is_empty());
    }

    #[test]
    fn close_all_closes_every_open_position() {
        let mut p = portfolio();
        for sym in ["AAPL", "GOOG", "MSFT"] {
            p.enter_long(Symbol::new(sym), date(2024, 1, 2), d("100"), d("5"));
        }
        assert_eq!(p.open_positions.len(), 3);
        let mut prices = HashMap::new();
        for sym in ["AAPL", "GOOG", "MSFT"] {
            prices.insert(Symbol::new(sym), d("110"));
        }
        p.close_all(date(2024, 1, 10), &prices);
        assert!(
            p.open_positions.is_empty(),
            "all positions should be closed"
        );
        assert_eq!(p.closed_trades.len(), 3);
    }
}
