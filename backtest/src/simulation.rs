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
        ((self.peak_value - current_value) / self.peak_value)
            .max(Decimal::ZERO)
    }

    /// Close all remaining open positions at the given prices (end-of-backtest).
    pub fn close_all(&mut self, date: NaiveDate, prices: &HashMap<Symbol, Decimal>) {
        let symbols: Vec<Symbol> = self.open_positions.keys().cloned().collect();
        for sym in symbols {
            let price = prices.get(&sym).copied().unwrap_or_else(|| {
                self.open_positions[&sym].entry_price
            });
            self.exit_position(&sym, date, price);
        }
    }
}
