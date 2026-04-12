use std::collections::HashMap;

use crate::config::Ticker;

use crate::backtest::state::EquityPoint;
use crate::backtest::types::TickerState;

pub struct EquityTracker {
    pub peak: f64,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub curve: Vec<EquityPoint>,
}

impl EquityTracker {
    pub fn new(starting_capital: f64) -> Self {
        Self {
            peak: starting_capital,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            curve: Vec::new(),
        }
    }

    pub fn update(&mut self, equity: f64, time: i64) {
        self.curve.push(EquityPoint { time, value: equity });
        if equity > self.peak {
            self.peak = equity;
        }
        let dd = self.peak - equity;
        let dd_pct = if self.peak > 0.0 { dd / self.peak } else { 0.0 };
        if dd > self.max_drawdown {
            self.max_drawdown = dd;
        }
        if dd_pct > self.max_drawdown_pct {
            self.max_drawdown_pct = dd_pct;
        }
    }

    /// Cash plus open-position mark-to-market across all tickers (portfolio book).
    pub fn portfolio_total(cash: f64, states: &HashMap<Ticker, TickerState>) -> f64 {
        cash + TickerState::sum_mark_to_market(states)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_tracks_peak_and_drawdown() {
        let mut t = EquityTracker::new(10_000.0);
        t.update(11_000.0, 1);
        assert!((t.peak - 11_000.0).abs() < 0.01);
        assert!((t.max_drawdown).abs() < 0.01);

        t.update(10_500.0, 2);
        assert!((t.peak - 11_000.0).abs() < 0.01);
        assert!((t.max_drawdown - 500.0).abs() < 0.01);

        t.update(11_500.0, 3);
        assert!((t.peak - 11_500.0).abs() < 0.01);
    }

    #[test]
    fn drawdown_pct() {
        let mut t = EquityTracker::new(10_000.0);
        t.update(10_000.0, 1);
        t.update(9_000.0, 2);
        assert!((t.max_drawdown_pct - 0.10).abs() < 0.001);
    }

    #[test]
    fn portfolio_equity_cash_only() {
        let states = HashMap::new();
        assert!((EquityTracker::portfolio_total(5_000.0, &states) - 5_000.0).abs() < 0.01);
    }
}
