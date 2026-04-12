use crate::config::BacktestConfig;
use crate::types::ToF64;

pub const TICK_SIZE: f64 = 0.01;

impl BacktestConfig {
    pub fn commission(&self, shares: u32, trade_value: f64) -> f64 {
        let raw = shares.to_f64() * self.commission_per_share;
        let floored = raw.max(self.commission_min);
        let max_cap = trade_value * 0.01;
        floored.min(max_cap)
    }

    pub fn slippage(&self) -> f64 {
        self.slippage_ticks * TICK_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bc() -> BacktestConfig {
        BacktestConfig {
            starting_capital: 100_000.0,
            commission_per_share: 0.005,
            commission_min: 1.00,
            slippage_ticks: 2.0,
            execution_delay_bars: 3,
        }
    }

    #[test]
    fn tick_size_is_correct() {
        assert!((TICK_SIZE - 0.01).abs() < 1e-10);
    }

    #[test]
    fn commission_per_share_above_minimum() {
        let bc = test_bc();
        let comm = bc.commission(400, 400.0 * 250.0);
        assert!((comm - 2.0).abs() < 1e-4);
    }

    #[test]
    fn commission_floors_at_minimum() {
        let bc = test_bc();
        let comm = bc.commission(10, 10.0 * 250.0);
        assert!((comm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn commission_caps_at_1pct_of_value() {
        let bc = test_bc();
        let comm = bc.commission(1, 0.5);
        assert!((comm - 0.005).abs() < 1e-6);
    }
}
