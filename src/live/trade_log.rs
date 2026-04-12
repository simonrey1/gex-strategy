use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use ts_rs::TS;

use crate::data::paths::data_dir;
use crate::types::Signal;

fn trade_log_path() -> PathBuf {
    data_dir().join("live").join("trade-log.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct TradeRecord {
    pub id: u32,
    pub ticker: String,
    pub signal: Signal,
    pub side: String,
    pub reason: String,
    pub shares: u32,
    pub price: f64,
    #[serde(rename = "stopLoss")]
    pub stop_loss: Option<f64>,
    #[serde(rename = "takeProfit")]
    pub take_profit: Option<f64>,
    pub pnl: Option<f64>,
    #[serde(rename = "returnPct")]
    pub return_pct: Option<f64>,
    pub equity: f64,
    pub timestamp: String,
}

pub fn read_trade_log() -> Vec<TradeRecord> {
    let path = trade_log_path();
    if !path.exists() {
        return vec![];
    }
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => vec![],
    }
}

pub fn append_trade(record: TradeRecord) -> Result<()> {
    let dir = data_dir().join("live");
    fs::create_dir_all(&dir)?;

    let mut trades = read_trade_log();
    trades.push(record);
    let json = serde_json::to_string_pretty(&trades)?;
    let path = trade_log_path();
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn next_trade_id() -> u32 {
    let trades = read_trade_log();
    trades.last().map(|t| t.id + 1).unwrap_or(1)
}

pub fn log_entry_trade(
    ticker: crate::config::Ticker,
    pos: &super::state::LivePosition,
    reason: &str,
    equity: f64,
) {
    if let Err(e) = append_trade(TradeRecord {
        id: next_trade_id(),
        ticker: ticker.as_str().to_string(),
        signal: pos.holding,
        side: "ENTRY".to_string(),
        reason: reason.to_string(),
        shares: pos.shares,
        price: pos.entry_price,
        stop_loss: Some(pos.stop_loss),
        take_profit: Some(pos.take_profit),
        pnl: None,
        return_pct: None,
        equity,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }) {
        eprintln!("[WARN] trade log ENTRY write failed for {}: {:?}", ticker, e);
    }
}

pub fn log_exit_trade(
    ticker: crate::config::Ticker,
    pos: &super::state::LivePosition,
    reason: &str,
    price: f64,
    pnl: f64,
    return_pct: f64,
    equity: f64,
) {
    if let Err(e) = append_trade(TradeRecord {
        id: next_trade_id(),
        ticker: ticker.as_str().to_string(),
        signal: pos.holding,
        side: "EXIT".to_string(),
        reason: reason.to_string(),
        shares: pos.shares,
        price,
        stop_loss: None,
        take_profit: None,
        pnl: Some(pnl),
        return_pct: Some(return_pct),
        equity,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }) {
        eprintln!("[WARN] trade log EXIT write failed for {}: {:?}", ticker, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_record_serde_roundtrip() {
        let r = TradeRecord {
            id: 1,
            ticker: "AAPL".into(),
            signal: Signal::LongVannaFlip,
            side: "ENTRY".into(),
            reason: "test".into(),
            shares: 10,
            price: 150.0,
            stop_loss: Some(145.0),
            take_profit: Some(160.0),
            pnl: None,
            return_pct: None,
            equity: 10_000.0,
            timestamp: "2025-01-01T10:00:00Z".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let de: TradeRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(de.ticker, "AAPL");
        assert_eq!(de.shares, 10);
        assert!((de.price - 150.0).abs() < 0.01);
        assert_eq!(de.stop_loss, Some(145.0));
        assert_eq!(de.pnl, None);
    }

    #[test]
    fn trade_record_exit_has_pnl() {
        let r = TradeRecord {
            id: 2,
            ticker: "GOOG".into(),
            signal: Signal::LongWallBounce,
            side: "EXIT".into(),
            reason: "stop_loss".into(),
            shares: 5,
            price: 140.0,
            stop_loss: None,
            take_profit: None,
            pnl: Some(-50.0),
            return_pct: Some(-3.3),
            equity: 9_950.0,
            timestamp: "2025-01-02T15:00:00Z".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let de: TradeRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(de.pnl, Some(-50.0));
        assert!(de.stop_loss.is_none());
    }
}
