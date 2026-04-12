use crate::types::{F64Trunc, ToF64};

/// Capital allocation inputs for [`PositionCash::entry_shares`].
#[derive(Debug, Clone, Copy)]
pub struct EntrySharesInputs {
    pub capital: f64,
    pub size_pct: f64,
    pub entry_price: f64,
}

impl EntrySharesInputs {
    #[inline]
    pub const fn new(capital: f64, size_pct: f64, entry_price: f64) -> Self {
        Self {
            capital,
            size_pct,
            entry_price,
        }
    }

    /// Floored share count (same rules as [`PositionCash::entry_shares`]).
    #[inline]
    pub fn floor_shares(&self) -> u32 {
        if self.entry_price <= 0.0 || !self.entry_price.is_finite() {
            return 0;
        }
        ((self.capital * self.size_pct) / self.entry_price).floor().trunc_u32()
    }
}

/// Entry / exit prices and size for [`PositionCash::exit_pnl`] / [`ExitPnlInputs::gross_pnl_and_return_pct`].
#[derive(Debug, Clone, Copy)]
pub struct ExitPnlInputs {
    pub entry_price: f64,
    pub exit_price: f64,
    pub shares: u32,
}

impl ExitPnlInputs {
    #[inline]
    pub const fn new(entry_price: f64, exit_price: f64, shares: u32) -> Self {
        Self {
            entry_price,
            exit_price,
            shares,
        }
    }

    /// Gross PnL and return % for a closed position.
    /// Commission/slippage are added by the caller (backtest only).
    #[inline]
    pub fn gross_pnl_and_return_pct(&self) -> (f64, f64) {
        let sh = self.shares.to_f64();
        let pnl = (self.exit_price - self.entry_price) * sh;
        let return_pct = if self.entry_price > 0.0 {
            (pnl / (self.entry_price * sh)) * 100.0
        } else {
            0.0
        };
        (pnl, return_pct)
    }
}

/// Stateless helpers for dollar sizing and exit PnL (live + backtest).
pub struct PositionCash;

impl PositionCash {
    #[inline]
    pub fn entry_shares(i: &EntrySharesInputs) -> u32 {
        i.floor_shares()
    }

    /// Gross PnL and return % for a closed position.
    #[inline]
    pub fn exit_pnl(i: &ExitPnlInputs) -> (f64, f64) {
        i.gross_pnl_and_return_pct()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_shares_basic() {
        let i = EntrySharesInputs::new(10_000.0, 0.30, 150.0);
        assert_eq!(PositionCash::entry_shares(&i), 20);
    }

    #[test]
    fn entry_shares_zero_price() {
        assert_eq!(PositionCash::entry_shares(&EntrySharesInputs::new(10_000.0, 0.30, 0.0)), 0);
        assert_eq!(PositionCash::entry_shares(&EntrySharesInputs::new(10_000.0, 0.30, -5.0)), 0);
        assert_eq!(PositionCash::entry_shares(&EntrySharesInputs::new(10_000.0, 0.30, f64::NAN)), 0);
    }

    #[test]
    fn entry_shares_floors() {
        assert_eq!(PositionCash::entry_shares(&EntrySharesInputs::new(10_000.0, 0.30, 333.0)), 9);
    }

    #[test]
    fn exit_pnl_profit() {
        let (pnl, pct) = ExitPnlInputs::new(100.0, 110.0, 10).gross_pnl_and_return_pct();
        assert!((pnl - 100.0).abs() < 0.01);
        assert!((pct - 10.0).abs() < 0.01);
    }

    #[test]
    fn exit_pnl_loss() {
        let (pnl, _pct) = PositionCash::exit_pnl(&ExitPnlInputs::new(100.0, 95.0, 20));
        assert!((pnl - -100.0).abs() < 0.01);
    }

    #[test]
    fn exit_pnl_zero_entry() {
        let (pnl, pct) = ExitPnlInputs::new(0.0, 50.0, 10).gross_pnl_and_return_pct();
        assert!((pct).abs() < 0.01);
        assert!(pnl > 0.0);
    }
}
