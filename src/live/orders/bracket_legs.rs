use crate::config::Ticker;

/// Position bracket parameters for SL/TP order placement.
pub struct BracketLegs {
    pub ticker: Ticker,
    pub shares: u32,
    pub sl_price: f64,
    pub tp_price: f64,
}
