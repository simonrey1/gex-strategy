use serde::Serialize;
use ts_rs::TS;

use crate::strategy::indicators::IndicatorValues;

// ─── IBKR position / order snapshots (served to dashboard) ──────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "IbkrPosition")]
pub struct IbkrPositionRow {
    pub symbol: String,
    pub shares: f64,
    #[serde(rename = "avgCost")]
    pub avg_cost: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "IbkrOrder")]
pub struct IbkrOrderRow {
    #[serde(rename = "orderId")]
    pub order_id: i32,
    pub symbol: String,
    pub action: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    pub quantity: f64,
    #[serde(rename = "limitPrice")]
    pub limit_price: Option<f64>,
    #[serde(rename = "stopPrice")]
    pub stop_price: Option<f64>,
    pub status: String,
    pub filled: f64,
    pub remaining: f64,
}

// ─── Per-ticker health state (internal, pushed from runner) ─────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TickerHealth {
    #[serde(rename = "lastPollMs")]
    pub last_poll_ms: u64,
    pub position: bool,
    pub signal: Option<crate::types::Signal>,
    #[serde(rename = "spotPrice")]
    pub spot_price: f64,
    #[serde(rename = "lastBarTime")]
    pub last_bar_time: Option<String>,
    #[serde(rename = "barsToday")]
    pub bars_today: u32,
    #[serde(rename = "consecutiveFailures")]
    pub consecutive_failures: u32,
    pub equity: f64,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    pub indicators: Option<TickerIndicators>,
    /// None = warmup done; Some("Fetching…") or Some("Replaying 1200/2304") during startup.
    #[serde(rename = "warmupStatus")]
    pub warmup_status: Option<String>,
}

impl Default for TickerHealth {
    fn default() -> Self {
        Self {
            last_poll_ms: 0,
            position: false,
            signal: None,
            spot_price: 0.0,
            last_bar_time: None,
            bars_today: 0,
            consecutive_failures: 0,
            equity: 0.0,
            last_error: None,
            indicators: None,
            warmup_status: None,
        }
    }
}

// ─── Indicator snapshot (served to dashboard per ticker) ────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct TickerIndicators {
    pub atr: f64,
    #[serde(rename = "emaFast")]
    pub ema_fast: f64,
    #[serde(rename = "emaSlow")]
    pub ema_slow: f64,
    pub adx: f64,
    pub tsi: f64,
    #[serde(rename = "tsiBullish")]
    pub tsi_bullish: bool,
}

impl TickerIndicators {
    pub fn from_values(v: &IndicatorValues) -> Self {
        Self {
            atr: v.atr,
            ema_fast: v.ema_fast,
            ema_slow: v.ema_slow,
            adx: v.adx,
            tsi: v.tsi,
            tsi_bullish: v.tsi_bullish,
        }
    }
}
