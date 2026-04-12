use anyhow::Result;

use crate::config::Ticker;

#[derive(Debug, Clone)]
pub struct AccountSummary {
    pub net_liquidation: f64,
    pub total_cash: f64,
}

#[derive(Debug, Clone)]
pub struct MarketOrder {
    pub ticker: Ticker,
    pub action: String,
    pub quantity: u32,
}

#[derive(Debug, Clone)]
pub struct BracketOrder {
    pub ticker: Ticker,
    pub action: String,
    pub quantity: u32,
    pub stop_loss_price: f64,
    pub take_profit_price: f64,
}

#[derive(Debug, Clone)]
pub struct BracketOrderIds {
    pub parent_id: i32,
    pub stop_loss_id: i32,
    pub take_profit_id: i32,
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub ticker: Ticker,
    pub action: String,
    pub order_id: i32,
    pub avg_price: f64,
    pub shares: u32,
}

/// Broker trait -- methods return boxed futures for dyn compatibility.
pub trait Broker: Send + Sync {
    fn connect(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn disconnect(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn is_connected(&self) -> bool;
    fn get_account_summary(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AccountSummary>> + Send + '_>>;
    fn place_market_order(&self, order: MarketOrder) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32>> + Send + '_>>;
    fn place_bracket_order(&self, order: BracketOrder) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<BracketOrderIds>> + Send + '_>>;
    fn cancel_order(&self, order_id: i32) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
}
