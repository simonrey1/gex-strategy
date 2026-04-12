use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::broker::types::BracketOrderIds;
use crate::config::Ticker;
use crate::live::orders::BracketLegs;
use crate::data::paths::data_dir;
use crate::strategy::engine::StrategyEngine;
use crate::strategy::signals::SignalState;
use crate::types::Signal;

const MAX_HISTORY_FILES: usize = 10_000;
const MAX_HISTORY_DAYS: u64 = 300;

fn state_dir() -> PathBuf {
    data_dir().join("live")
}

fn history_dir() -> PathBuf {
    state_dir().join("history")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTradingState {
    pub version: u32,
    #[serde(rename = "savedAt")]
    pub saved_at: String,
    pub ticker: String,
    pub position: Option<LivePosition>,
    #[serde(rename = "signalState")]
    pub signal_state: SerializedSignalState,
    #[serde(rename = "lastBarTimestamp")]
    pub last_bar_timestamp: String,
    #[serde(rename = "dailyRealizedPnl")]
    pub daily_realized_pnl: f64,
    #[serde(rename = "dailyEntries")]
    pub daily_entries: u32,
    #[serde(rename = "stopLossOrderId", default)]
    pub stop_loss_order_id: Option<i32>,
    #[serde(rename = "takeProfitOrderId", default)]
    pub take_profit_order_id: Option<i32>,
}

impl LiveTradingState {
    /// IBKR bracket SL/TP ids when both are present (for reconcile after restart).
    pub fn bracket_order_ids(&self) -> Option<BracketOrderIds> {
        match (self.stop_loss_order_id, self.take_profit_order_id) {
            (Some(sl), Some(tp)) => Some(BracketOrderIds {
                parent_id: 0,
                stop_loss_id: sl,
                take_profit_id: tp,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePosition {
    pub holding: Signal,
    pub shares: u32,
    #[serde(rename = "entryPrice")]
    pub entry_price: f64,
    #[serde(rename = "entryTime")]
    pub entry_time: String,
    #[serde(rename = "stopLoss")]
    pub stop_loss: f64,
    #[serde(rename = "takeProfit")]
    pub take_profit: f64,
    /// ATR at entry, used for wall-hop SL computation.
    #[serde(rename = "entryAtr", default)]
    pub entry_atr: f64,
    #[serde(rename = "highestPutWall", default)]
    pub highest_put_wall: f64,
    #[serde(rename = "highestClose", default)]
    pub highest_close: f64,
    #[serde(rename = "hurstExhaustBars", default)]
    pub hurst_exhaust_bars: u32,
}

impl LivePosition {
    /// SL/TP + size for GTC bracket placement (reconcile path).
    #[inline]
    pub fn bracket_legs(&self, ticker: Ticker) -> BracketLegs {
        BracketLegs {
            ticker,
            shares: self.shares,
            sl_price: self.stop_loss,
            tp_price: self.take_profit,
        }
    }
}

impl crate::strategy::engine::HasTrailFields for LivePosition {
    fn trail_fields(&mut self) -> crate::strategy::engine::TrailFields<'_> {
        crate::strategy::engine::TrailFields {
            stop_loss: &mut self.stop_loss,
            highest_put_wall: &mut self.highest_put_wall,
            highest_close: &mut self.highest_close,
            hurst_exhaust_bars: &mut self.hurst_exhaust_bars,
            entry_price: self.entry_price,
            tp: self.take_profit,
            signal: self.holding,
        }
    }
}

/// Persisted signal state — position fields only.
/// Everything else (GEX tracker, zone score, IV baseline/spike, EOD state,
/// prev_eod_pw, bar_index) is rebuilt by running `generate_signal` over
/// historical IBKR bars + ThetaData GEX profiles during `replay_warmup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedSignalState {
    /// Current position type (Flat if no position).
    pub holding: Signal,
    /// Entry price of current position (0.0 if flat).
    #[serde(rename = "entryPrice")]
    pub entry_price: f64,
}

fn state_path(ticker: Ticker) -> PathBuf {
    state_dir().join(format!("state-{}.json", ticker))
}

pub fn load_live_state(ticker: Ticker) -> Option<LiveTradingState> {
    let path = state_path(ticker);
    if !path.exists() {
        return None;
    }
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[live] Failed to read state file for {}: {}", ticker, e);
            return None;
        }
    };
    match serde_json::from_str(&data) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[live] Corrupt state JSON for {} — ignoring: {}", ticker, e);
            None
        }
    }
}

pub fn persist_live_state(
    ticker: Ticker,
    signal_state: &SignalState,
    position: Option<&LivePosition>,
    last_bar_timestamp: &str,
    daily_realized_pnl: f64,
    daily_entries: u32,
    stop_loss_order_id: Option<i32>,
    take_profit_order_id: Option<i32>,
) -> Result<()> {
    fs::create_dir_all(state_dir())?;

    let state = LiveTradingState {
        version: 2,
        saved_at: chrono::Utc::now().to_rfc3339(),
        ticker: ticker.as_str().to_string(),
        position: position.cloned(),
        signal_state: SerializedSignalState {
            holding: signal_state.holding,
            entry_price: signal_state.entry_price,
        },
        last_bar_timestamp: last_bar_timestamp.to_string(),
        daily_realized_pnl,
        daily_entries,
        stop_loss_order_id,
        take_profit_order_id,
    };

    let json = serde_json::to_string_pretty(&state)?;
    let path = state_path(ticker);

    // Back up the current state before overwriting
    if path.exists() {
        backup_state_file(ticker, &path);
    }

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn backup_state_file(ticker: Ticker, current: &PathBuf) {
    let dir = history_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let dest = dir.join(format!("state-{}-{}.json", ticker, ts));
    if let Err(e) = fs::copy(current, &dest) {
        eprintln!("[live] State backup failed for {}: {}", ticker, e);
    }

    cleanup_history(ticker);
}

/// Delete history files that exceed BOTH thresholds:
/// keep if within last MAX_HISTORY_FILES OR younger than MAX_HISTORY_DAYS.
fn cleanup_history(ticker: Ticker) {
    let dir = history_dir();
    let prefix = format!("state-{}-", ticker);

    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((e.path(), meta.modified().ok()?))
            })
            .collect(),
        Err(_) => return,
    };

    if entries.len() <= MAX_HISTORY_FILES {
        return;
    }

    entries.sort_by(|a, b| b.1.cmp(&a.1));

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(MAX_HISTORY_DAYS * 86_400);

    for (path, mtime) in entries.iter().skip(MAX_HISTORY_FILES) {
        if *mtime < cutoff {
            if let Err(e) = fs::remove_file(path) {
                eprintln!("[live] Failed to clean up history file {:?}: {}", path, e);
            }
        }
    }
}

/// Groups the mutable tracking state that the live runner persists each poll.
pub struct RunnerSnapshot<'a> {
    pub engine: &'a StrategyEngine,
    pub position: &'a Option<LivePosition>,
    pub bracket_ids: &'a Option<crate::broker::types::BracketOrderIds>,
    pub last_processed_ms: i64,
    pub daily: &'a crate::strategy::engine::DailyState,
}

impl RunnerSnapshot<'_> {
    /// Persist all state atomically.
    pub fn save(&self, ticker: Ticker) {
        if self.last_processed_ms == 0 {
            return;
        }

        let last_ts = chrono::DateTime::from_timestamp_millis(self.last_processed_ms)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        let (sl_id, tp_id) = self.bracket_ids.as_ref()
            .map(|bi| (Some(bi.stop_loss_id), Some(bi.take_profit_id)))
            .unwrap_or((None, None));

        if let Err(e) = persist_live_state(
            ticker,
            &self.engine.signal_state,
            self.position.as_ref(),
            &last_ts,
            self.daily.realized_pnl,
            self.daily.entries,
            sl_id,
            tp_id,
        ) {
            eprintln!("[live-{}] Failed to persist state: {:?}", ticker, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_signal_state_roundtrip() {
        let signal_state = SerializedSignalState {
            holding: Signal::LongVannaFlip,
            entry_price: 142.50,
        };
        let json = serde_json::to_string(&signal_state).unwrap();
        let de: SerializedSignalState = serde_json::from_str(&json).unwrap();
        assert_eq!(de.holding, Signal::LongVannaFlip);
        assert!((de.entry_price - 142.50).abs() < 1e-10);
    }

    #[test]
    fn serialized_signal_state_flat_roundtrip() {
        let signal_state = SerializedSignalState {
            holding: Signal::Flat,
            entry_price: 0.0,
        };
        let json = serde_json::to_string(&signal_state).unwrap();
        let de: SerializedSignalState = serde_json::from_str(&json).unwrap();
        assert_eq!(de.holding, Signal::Flat);
        assert_eq!(de.entry_price, 0.0);
    }

    #[test]
    fn old_state_json_with_extra_fields_deserializes() {
        let json = r#"{
            "holding": "FLAT",
            "entryPrice": 0.0,
            "barIndex": 500,
            "entryBar": 42,
            "ivSpikeLevel": 0.3,
            "gexTrackerBuf": [1.0, 2.0]
        }"#;
        let de: SerializedSignalState = serde_json::from_str(json).unwrap();
        assert_eq!(de.holding, Signal::Flat);
        assert_eq!(de.entry_price, 0.0);
    }
}
