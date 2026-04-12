/// Tracks per-day risk counters. Used by both runners to ensure identical
/// daily-loss-limit and max-entries-per-day enforcement.
pub struct DailyState {
    pub realized_pnl: f64,
    pub loss_limit_hit: bool,
    pub entries: u32,
    loss_limit_pct: f64,
}

impl DailyState {
    pub fn new(loss_limit_pct: f64) -> Self {
        Self { realized_pnl: 0.0, loss_limit_hit: false, entries: 0, loss_limit_pct }
    }

    pub fn reset(&mut self) {
        self.realized_pnl = 0.0;
        self.loss_limit_hit = false;
        self.entries = 0;
    }

    /// Record an exit's PnL and check if the daily loss limit has been breached.
    pub fn record_exit(&mut self, pnl: f64, equity: f64) {
        self.realized_pnl += pnl;
        if self.realized_pnl <= -(equity * self.loss_limit_pct) {
            self.loss_limit_hit = true;
        }
    }

    /// Whether daily risk limits allow another entry.
    pub fn allows_entry(&self, max_entries: u32) -> bool {
        !self.loss_limit_hit && self.entries < max_entries
    }
}

impl Default for DailyState {
    fn default() -> Self {
        Self::new(0.15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_state_loss_limit() {
        let mut ds = DailyState::new(0.02);
        assert!(!ds.loss_limit_hit);
        ds.record_exit(-150.0, 10_000.0);
        assert!(!ds.loss_limit_hit);
        ds.record_exit(-60.0, 10_000.0);
        assert!(ds.loss_limit_hit);
    }

    #[test]
    fn daily_state_reset() {
        let mut ds = DailyState::new(0.02);
        ds.record_exit(-300.0, 10_000.0);
        assert!(ds.loss_limit_hit);
        ds.entries = 5;
        ds.reset();
        assert!(!ds.loss_limit_hit);
        assert_eq!(ds.entries, 0);
        assert!((ds.realized_pnl).abs() < 0.01);
    }
}
