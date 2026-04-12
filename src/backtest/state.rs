use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use ts_rs::TS;

use super::iv_scan::IvScanResult;
use super::metrics::BacktestResult;

use crate::data::paths::data_dir;

fn results_dir() -> PathBuf {
    data_dir().join("results")
}
const RETENTION_DAYS: u64 = 10;

#[derive(serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "SavedBacktestState")]
pub struct SavedState<'a> {
    version: u32,
    #[serde(rename = "savedAt")]
    saved_at: String,
    result: &'a BacktestResult,
    #[serde(rename = "chartData", skip_serializing_if = "Option::is_none")]
    chart_data: Option<&'a ChartData>,
    #[serde(rename = "ivScan", skip_serializing_if = "Option::is_none")]
    iv_scan: Option<&'a [IvScanResult]>,
}

impl<'a> SavedState<'a> {
    pub fn new(
        result: &'a BacktestResult,
        chart_data: Option<&'a ChartData>,
        iv_scan: &'a [IvScanResult],
    ) -> Self {
        Self {
            version: 1,
            saved_at: chrono::Utc::now().to_rfc3339(),
            result,
            chart_data,
            iv_scan: if iv_scan.is_empty() { None } else { Some(iv_scan) },
        }
    }

    pub fn write(&self, ticker_name: &str) -> Result<String> {
        let dir = results_dir();
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)?;

        let ts = chrono::Utc::now().format("%Y-%m-%d-%H-%M-%S").to_string();
        let filepath = dir.join(format!("state-{}-{}.json", ticker_name, ts));
        fs::write(&filepath, &json)?;
        fs::write(dir.join(format!("latest-{}.json", ticker_name)), &json)?;
        fs::write(dir.join("latest.json"), &json)?;
        prune_old_files();
        Ok(filepath.display().to_string())
    }
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct ChartBar {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct WallPoint {
    pub time: i64,
    pub value: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/", rename = "ChartMarker")]
pub struct Marker {
    pub time: i64,
    pub position: String,
    pub color: String,
    pub shape: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EquityPoint {
    pub time: i64,
    pub value: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct WallBand {
    pub strike: f64,
    /// gamma_oi / total_gamma_oi for this side (0..1).
    pub pct: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct WallBandBar {
    pub time: i64,
    #[serde(rename = "putWalls")]
    pub put_walls: Vec<WallBand>,
    #[serde(rename = "callWalls")]
    pub call_walls: Vec<WallBand>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct ChartData {
    pub bars: Vec<ChartBar>,
    #[serde(rename = "putWalls")]
    pub put_walls: Vec<WallPoint>,
    #[serde(rename = "callWalls")]
    pub call_walls: Vec<WallPoint>,
    pub markers: Vec<Marker>,
    /// Mid-distance put wall (>1% OTM). Used for WB zone tracking.
    #[serde(rename = "midPutWall", default, skip_serializing_if = "Vec::is_empty")]
    pub mid_put_wall: Vec<WallPoint>,
    /// Structural put wall from wide-strike daily snapshot (±25% OTM).
    #[serde(rename = "widePutWall", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_put_wall: Vec<WallPoint>,
    /// Structural call wall from wide-strike daily snapshot (±25% OTM).
    /// Dashed on chart — shows structural resistance / VannaFlip TP target.
    #[serde(rename = "wideCallWall", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_call_wall: Vec<WallPoint>,
    /// Smoothed put wall (peak-hold with decay). Only populated when wall_smooth_halflife > 0.
    #[serde(rename = "smoothPutWall", default, skip_serializing_if = "Vec::is_empty")]
    pub smooth_put_wall: Vec<WallPoint>,
    /// Smoothed call wall (trough-hold with decay). Only populated when wall_smooth_halflife > 0.
    #[serde(rename = "smoothCallWall", default, skip_serializing_if = "Vec::is_empty")]
    pub smooth_call_wall: Vec<WallPoint>,
    /// Smoothed highest narrow put wall (EMA of max strike among top 2).
    #[serde(rename = "smoothHighestPw", default, skip_serializing_if = "Vec::is_empty")]
    pub smooth_highest_pw: Vec<WallPoint>,
    /// Smoothed lowest narrow call wall (EMA of min strike among top 2).
    #[serde(rename = "smoothLowestCw", default, skip_serializing_if = "Vec::is_empty")]
    pub smooth_lowest_cw: Vec<WallPoint>,
    /// Spread-gate smoothed put wall (pure EMA, no γ×OI weighting).
    #[serde(rename = "spreadPutWall", default, skip_serializing_if = "Vec::is_empty")]
    pub spread_put_wall: Vec<WallPoint>,
    /// Spread-gate smoothed call wall (pure EMA, no γ×OI weighting).
    #[serde(rename = "spreadCallWall", default, skip_serializing_if = "Vec::is_empty")]
    pub spread_call_wall: Vec<WallPoint>,
    /// Put wall ranks 2-5 (top 5 narrow put walls by γ×OI, rank 1 = putWalls).
    #[serde(rename = "putWall2", default, skip_serializing_if = "Vec::is_empty")]
    pub put_wall_2: Vec<WallPoint>,
    #[serde(rename = "putWall3", default, skip_serializing_if = "Vec::is_empty")]
    pub put_wall_3: Vec<WallPoint>,
    #[serde(rename = "putWall4", default, skip_serializing_if = "Vec::is_empty")]
    pub put_wall_4: Vec<WallPoint>,
    #[serde(rename = "putWall5", default, skip_serializing_if = "Vec::is_empty")]
    pub put_wall_5: Vec<WallPoint>,
    /// Call wall ranks 2-5 (top 5 narrow call walls by γ×OI, rank 1 = callWalls).
    #[serde(rename = "callWall2", default, skip_serializing_if = "Vec::is_empty")]
    pub call_wall_2: Vec<WallPoint>,
    #[serde(rename = "callWall3", default, skip_serializing_if = "Vec::is_empty")]
    pub call_wall_3: Vec<WallPoint>,
    #[serde(rename = "callWall4", default, skip_serializing_if = "Vec::is_empty")]
    pub call_wall_4: Vec<WallPoint>,
    #[serde(rename = "callWall5", default, skip_serializing_if = "Vec::is_empty")]
    pub call_wall_5: Vec<WallPoint>,
    /// Top-5 put/call walls with γ×OI concentration per bar (for proportional rendering).
    #[serde(rename = "wallBands", default, skip_serializing_if = "Vec::is_empty")]
    pub wall_bands: Vec<WallBandBar>,
    /// Per-bar tooltip lines during active IV spike windows.
    #[serde(rename = "spikeTooltips", default, skip_serializing_if = "Vec::is_empty")]
    pub spike_tooltips: Vec<BarTooltip>,
    /// Spike window boundaries [start_sec, end_sec] for full-height chart shading.
    #[serde(rename = "spikeWindows", default, skip_serializing_if = "Vec::is_empty")]
    pub spike_windows: Vec<SpikeWindow>,
    /// Fast EMA of ATM put IV (5-bar on 15-min).
    #[serde(rename = "ivEmaFast", default, skip_serializing_if = "Vec::is_empty")]
    pub iv_ema_fast: Vec<WallPoint>,
    /// Slow EMA of ATM put IV (15-bar on 15-min).
    #[serde(rename = "ivEmaSlow", default, skip_serializing_if = "Vec::is_empty")]
    pub iv_ema_slow: Vec<WallPoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct SpikeWindow {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct BarTooltip {
    pub time: i64,
    pub lines: Vec<String>,
}

impl ChartData {

    /// Sort all series by time and deduplicate bars (lightweight-charts
    /// requires strictly ascending timestamps with no duplicates).
    pub fn finalize(&mut self) {
        dedup_bars(&mut self.bars);
        dedup_wall(&mut self.put_walls);
        dedup_wall(&mut self.call_walls);
        dedup_wall(&mut self.mid_put_wall);
        dedup_wall(&mut self.wide_put_wall);
        dedup_wall(&mut self.wide_call_wall);
        dedup_wall(&mut self.smooth_put_wall);
        dedup_wall(&mut self.smooth_call_wall);
        dedup_wall(&mut self.smooth_highest_pw);
        dedup_wall(&mut self.smooth_lowest_cw);
        dedup_wall(&mut self.spread_put_wall);
        dedup_wall(&mut self.spread_call_wall);
        for v in [&mut self.put_wall_2, &mut self.put_wall_3, &mut self.put_wall_4, &mut self.put_wall_5,
                   &mut self.call_wall_2, &mut self.call_wall_3, &mut self.call_wall_4, &mut self.call_wall_5] {
            dedup_wall(v);
        }
        dedup_wall(&mut self.iv_ema_fast);
        dedup_wall(&mut self.iv_ema_slow);
        self.markers.sort_by_key(|m| m.time);
    }
}

fn dedup_bars(bars: &mut Vec<ChartBar>) {
    bars.sort_by_key(|b| b.time);
    bars.dedup_by_key(|b| b.time);
}

fn dedup_wall(pts: &mut Vec<WallPoint>) {
    pts.sort_by_key(|p| p.time);
    pts.dedup_by_key(|p| p.time);
}

impl BacktestResult {
    pub fn save_state(
        &self,
        chart_data: Option<&ChartData>,
        iv_scan: &[IvScanResult],
    ) -> Result<String> {
        SavedState::new(self, chart_data, iv_scan).write(self.ticker.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Ticker;
    use crate::backtest::backtest_result::BacktestResult;

    fn minimal_result() -> BacktestResult {
        BacktestResult {
            ticker: Ticker::AAPL,
            label: "AAPL".into(),
            start_date: "2024-01-01".into(),
            end_date: "2024-12-31".into(),
            total_bars: 1000,
            total_trades: 0,
            winners: 0,
            losers: 0,
            win_rate: 0.0,
            gross_pnl: 0.0,
            total_commission: 0.0,
            total_slippage: 0.0,
            net_pnl: 0.0,
            total_return_pct: 0.0,
            profit_factor: None,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            sharpe_ratio: 0.0,
            sortino_ratio: 0.0,
            calmar_ratio: 0.0,
            cagr: 0.0,
            expectancy: 0.0,
            payoff_ratio: None,
            ulcer_index: 0.0,
            max_dd_duration_days: 0,
            avg_trade_duration_minutes: 0.0,
            avg_win_pct: 0.0,
            avg_loss_pct: 0.0,
            avg_trade_pct: 0.0,
            buy_hold_return_pct: 0.0,
            alpha_pct: 0.0,
            avg_capital_util_pct: 0.0,
            monthly_returns: vec![],
            trades: vec![],
            trade_analysis: Default::default(),
            wall_events: vec![],
        }
    }

    #[test]
    fn saved_state_new_without_iv_scan() {
        let r = minimal_result();
        let state = SavedState::new(&r, None, &[]);
        assert_eq!(state.version, 1);
        assert!(state.iv_scan.is_none());
        assert!(state.chart_data.is_none());
    }

    #[test]
    fn saved_state_serializes_to_json() {
        let r = minimal_result();
        let state = SavedState::new(&r, None, &[]);
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["result"]["ticker"], "AAPL");
        assert!(json["savedAt"].as_str().unwrap().contains("T"));
        assert!(json.get("ivScan").is_none());
    }

    #[test]
    fn saved_state_includes_chart_data() {
        let r = minimal_result();
        let cd = ChartData {
            bars: vec![ChartBar { time: 100, open: 1.0, high: 2.0, low: 0.5, close: 1.5 }],
            ..Default::default()
        };
        let state = SavedState::new(&r, Some(&cd), &[]);
        let json = serde_json::to_value(&state).unwrap();
        assert!(json["chartData"]["bars"].as_array().unwrap().len() == 1);
    }
}

fn prune_old_files() {
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(RETENTION_DAYS * 24 * 3600);

    if let Ok(entries) = fs::read_dir(results_dir()) {
        let mut pruned = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with("state-") || !name_str.ends_with(".json") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        fs::remove_file(entry.path()).ok();
                        pruned += 1;
                    }
                }
            }
        }
        if pruned > 0 {
            println!("[state] Pruned {} files older than {} days", pruned, RETENTION_DAYS);
        }
    }
}
