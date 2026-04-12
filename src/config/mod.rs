pub mod bar_interval;
pub mod strategy;
pub mod bar_counts;
pub mod tickers;

pub use bar_interval::{strategy_bars_from_1m_count, Minutes, BAR_INTERVAL_MINUTES};
pub use bar_counts::{BarIndex, IV_LOOKBACK_BARS_INDEX};
pub use strategy::{BacktestConfig, StrategyConfig};
pub use tickers::Ticker;

pub const DEFAULT_START: &str = "2018-01-01";
pub const DEFAULT_END: &str = "2026-04-01";

pub fn thetadata_host() -> String {
    std::env::var("THETADATA_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn thetadata_port() -> u16 {
    std::env::var("THETADATA_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(25503)
}

pub fn health_port() -> u16 {
    std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080)
}

pub fn ibkr_host() -> String {
    std::env::var("IBKR_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn ibkr_port() -> u16 {
    std::env::var("IBKR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4002)
}

fn random_client_id(lo: i32, hi: i32) -> i32 {
    let seed = (std::process::id() as i32).wrapping_mul(31)
        ^ (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as i32);
    let range = (hi - lo) as u32;
    lo + (seed.unsigned_abs() % range) as i32
}

pub fn ibkr_client_id_live() -> i32 {
    random_client_id(100, 499)
}

pub fn ibkr_client_id_backtest() -> i32 {
    random_client_id(500, 999)
}
