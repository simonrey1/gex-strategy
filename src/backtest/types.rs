use std::collections::HashMap;

use ts_rs::TS;

use crate::config::strategy::{HURST_WINDOW, WALL_SMOOTH_HALFLIFE};
use crate::config::{BacktestConfig, BarIndex, StrategyConfig, Ticker};
use crate::strategy::engine::{DailyState, EntryCandidateData, StrategyEngine};
use crate::strategy::hurst::HurstTracker;
use crate::strategy::shared::GexPipelineBar;
use crate::strategy::wall_trail::WallTrailOutcome;
use crate::strategy::slot_sizing::{
    EntryPrepareCtx, EntryPrepareInputs, EntryRegimeFields, PrepareEntryError, PreparedEntry,
};
use crate::strategy::wall_smoother::WallSmoother;
use crate::types::{GexProfile, OhlcBar, ToF64, TradeSignal};

use super::iv_scan::IvScanResult;
use super::metrics::EtFormat;
use super::positions::*;
use super::state::*;

/// One closed trade + bar timestamp + portfolio starting capital for [`TickerState::record_trade`].
pub struct RecordTradeInputs {
    pub trade: Trade,
    pub bar_time_sec: i64,
    pub starting_capital: f64,
}

/// Closed trade + bar time + equity + starting capital for [`TickerState::record_exit`].
pub struct ChartExitRecord {
    pub trade: Trade,
    pub bar_time_sec: i64,
    pub equity: f64,
    pub starting_capital: f64,
}

pub struct TickerState {
    pub engine: StrategyEngine,
    pub position: Option<Position>,
    pub pending_entry: Option<PendingEntry>,
    pub position_diag: Option<EntryDiag>,
    /// (time_sec, close) of first post-warmup bar with GEX — used for buy-and-hold.
    pub first_trading_bar: Option<(i64, f64)>,
    pub last_bar: Option<OhlcBar>,
    pub trades: Vec<Trade>,
    pub chart_data: ChartData,
    pub equity_timeline: Vec<(i64, f64)>,
    pub cumulative_pnl: f64,
    pub peak_pnl: f64,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub daily: DailyState,
    pub split_logged: bool,
    /// Current day's split ratio — used to reverse-adjust chart data to post-split basis.
    pub chart_split_ratio: f64,
    pub gex_bars: u32,
    pub hurst: HurstTracker,
    pub wall_smoother: WallSmoother,
    /// Last iv_spike_bar value we emitted a marker for, to avoid duplicates.
    pub last_iv_spike_bar_marked: BarIndex,
    /// Previous iv_cross_dir for detecting crossover transitions.
    pub prev_iv_cross_dir: i8,
    /// IV scan hypothetical trade results (populated after finalize).
    pub iv_scan_results: Vec<IvScanResult>,
    /// Whether to collect chart data (markers, bars, walls, IV, tooltips).
    pub save_chart: bool,
}

impl TickerState {
    pub fn new(config: &StrategyConfig) -> Self {
        Self {
            engine: StrategyEngine::new(config),
            position: None,
            pending_entry: None,
            position_diag: None,
            first_trading_bar: None,
            last_bar: None,
            trades: Vec::new(),
            chart_data: ChartData::default(),
            equity_timeline: Vec::new(),
            cumulative_pnl: 0.0,
            peak_pnl: 0.0,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            daily: DailyState::new(config.daily_loss_limit_pct),
            split_logged: false,
            chart_split_ratio: 1.0,
            gex_bars: 0,
            hurst: HurstTracker::new(HURST_WINDOW),
            wall_smoother: WallSmoother::with_spread_halflife(WALL_SMOOTH_HALFLIFE, config.spread_smooth_halflife),
            last_iv_spike_bar_marked: -1,
            prev_iv_cross_dir: 0,
            iv_scan_results: Vec::new(),
            save_chart: false,
        }
    }

    pub fn record_trade(&mut self, i: RecordTradeInputs) {
        self.cumulative_pnl += i.trade.net_pnl;
        let equity = i.starting_capital + self.cumulative_pnl;
        self.equity_timeline.push((i.bar_time_sec, equity));
        if self.cumulative_pnl > self.peak_pnl {
            self.peak_pnl = self.cumulative_pnl;
        }
        let dd = self.peak_pnl - self.cumulative_pnl;
        if dd > self.max_drawdown {
            self.max_drawdown = dd;
            self.max_drawdown_pct = if i.starting_capital + self.peak_pnl > 0.0 {
                dd / (i.starting_capital + self.peak_pnl)
            } else {
                0.0
            };
        }
        self.trades.push(i.trade);
    }

    pub fn reset_daily(&mut self) {
        self.daily.reset();
    }

    pub fn mark_to_market(&self) -> f64 {
        self.position.as_ref().map(|p| {
            let close = self.last_bar.as_ref().map(|b| b.close).unwrap_or(p.entry_price);
            p.shares.to_f64() * close
        }).unwrap_or(0.0)
    }

    /// Open position or pending deferred entry consumes one portfolio slot.
    #[inline]
    pub fn slot_held(&self) -> bool {
        self.position.is_some() || self.pending_entry.is_some()
    }

    /// [`StrategyEngine::process_gex_bar_pipeline`] with this ticker’s smoother, hurst, and position.
    #[inline]
    pub fn run_gex_bar_pipeline(
        &mut self,
        pipe: GexPipelineBar<'_>,
        config: &StrategyConfig,
    ) -> (TradeSignal, WallTrailOutcome, Option<EntryCandidateData>) {
        self.engine.process_gex_bar_pipeline(
            &mut self.wall_smoother,
            &mut self.hurst,
            self.position.as_mut(),
            pipe,
            config,
            &self.daily,
        )
    }

    /// Total mark-to-market across all tickers (cash not included).
    pub fn sum_mark_to_market(states: &HashMap<Ticker, TickerState>) -> f64 {
        states.values().map(|ts| ts.mark_to_market()).sum()
    }
}

// ─── Deferred order types ───────────────────────────────────────────────────

pub struct PendingEntry {
    /// Frozen at signal time — same bundle as [`EntryCandidateData::entry_prepare_inputs`].
    pub prepare: EntryPrepareInputs,
    pub bars_left: i32,
    pub diag: EntryDiag,
    /// Spot at signal time, used for GEX normalization.
    pub gex_spot: f64,
    /// SignalState.iv_spike_bar at signal time — used for scan matching.
    pub spike_bar: BarIndex,
}

impl PendingEntry {
    /// Build a PendingEntry from the shared EntryCandidateData + backtest-specific fields.
    pub fn from_candidate(
        c: EntryCandidateData,
        gex: &GexProfile,
        bar: &OhlcBar,
        bc: &BacktestConfig,
        atr_regime_ratio: f64,
        spike_bar: BarIndex,
    ) -> Self {
        let prepare = c.entry_prepare_inputs(atr_regime_ratio);
        let pw = gex.pw_opt();
        let cw = gex.cw_opt();

        Self {
            prepare,
            bars_left: bc.execution_delay_bars,
            gex_spot: c.gex_spot,
            spike_bar,
            diag: EntryDiag::from_deferred_snapshot(&prepare, c, gex, bar, pw, cw),
        }
    }

    /// Frozen regime triple + signal/tp_cap (same as `prepare.regime` / `prepare.signal`).
    #[inline]
    pub fn regime(&self) -> EntryRegimeFields {
        self.prepare.regime
    }

    /// Same as [`EntryPrepareInputs::bundle_at_price`] with frozen signal-time inputs — fill at `trade_price` (bar open + slippage).
    #[inline]
    pub fn prepare_bundle_at_trade_price(
        &self,
        trade_price: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<(EntryPrepareInputs, PreparedEntry), PrepareEntryError> {
        self.prepare.bundle_at_price(trade_price, ctx)
    }
}

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct EntryDiag {
    #[serde(rename = "entryReason")]
    pub entry_reason: String,
    #[serde(rename = "entryPutWall")]
    pub entry_put_wall: Option<f64>,
    #[serde(rename = "entryCallWall")]
    pub entry_call_wall: Option<f64>,
    #[serde(rename = "entryNetGex")]
    pub entry_net_gex: f64,
    #[serde(rename = "entryZoneScore")]
    pub entry_zone_score: f64,
    #[serde(rename = "entryAtr")]
    pub entry_atr: f64,
    #[serde(rename = "entryAdx")]
    pub entry_adx: f64,
    #[serde(rename = "signalBarTs")]
    pub signal_bar_ts: String,
    #[serde(rename = "signalBarClose")]
    pub signal_bar_close: f64,
    #[serde(rename = "entryTsi")]
    pub entry_tsi: f64,
    #[serde(rename = "entryPcGammaRatio")]
    pub pc_gamma_ratio: f64,
    #[serde(rename = "entryAtmGammaDom")]
    pub atm_gamma_dom: f64,
    #[serde(rename = "entryNearGammaImbal")]
    pub near_gamma_imbal: f64,
    #[serde(rename = "entryPwComDistPct")]
    pub pw_com_dist_pct: f64,
    #[serde(rename = "entryPwNearFarRatio")]
    pub pw_near_far_ratio: f64,
    #[serde(rename = "entryPwDispersionAtr")]
    pub pw_dispersion_atr: f64,
    #[serde(rename = "entryCwDispersionAtr")]
    pub cw_dispersion_atr: f64,
}

impl EntryDiag {
    /// Chart/export snapshot; `entry_atr` / `entry_tsi` match `prepare.regime` (no second source of truth).
    pub(crate) fn from_deferred_snapshot(
        prepare: &EntryPrepareInputs,
        c: EntryCandidateData,
        gex: &GexProfile,
        bar: &OhlcBar,
        pw: Option<f64>,
        cw: Option<f64>,
    ) -> Self {
        let atr_tsi = prepare.regime.to_atr_tsi();
        let atr = atr_tsi.atr;
        Self {
            entry_reason: c.reason,
            entry_put_wall: pw,
            entry_call_wall: cw,
            entry_net_gex: c.net_gex,
            entry_zone_score: 0.0,
            entry_atr: atr_tsi.atr,
            entry_adx: c.adx,
            signal_bar_ts: EtFormat::utc(&bar.timestamp),
            signal_bar_close: bar.close,
            entry_tsi: atr_tsi.tsi,
            pc_gamma_ratio: if gex.total_call_goi > 0.0 { gex.total_put_goi / gex.total_call_goi } else { 0.0 },
            atm_gamma_dom: gex.atm_gamma_dominance,
            near_gamma_imbal: gex.near_gamma_imbalance,
            pw_com_dist_pct: gex.pw_com_dist_pct,
            pw_near_far_ratio: gex.pw_near_far_ratio,
            pw_dispersion_atr: gex.pw_dispersion_atr(atr),
            cw_dispersion_atr: GexProfile::weighted_wall_dispersion(&gex.call_walls, atr),
        }
    }

    pub fn finalize(&self, gex: Option<&GexProfile>, entry_price: f64) -> TradeDiagnostics {
        let exit_call_wall = gex.and_then(|g| g.cw_opt());
        TradeDiagnostics {
            entry: self.clone(),
            exit_put_wall: gex.and_then(|g| g.pw_opt()),
            exit_call_wall,
            exit_net_gex: gex.map(|g| g.net_gex).unwrap_or(0.0),
            call_wall_below_entry: exit_call_wall.is_some_and(|cw| cw <= entry_price),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Signal;
    use chrono::TimeZone;

    fn make_pos(shares: u32, entry: f64) -> Position {
        Position {
            signal: Signal::LongVannaFlip,
            entry_time: chrono::Utc.with_ymd_and_hms(2025, 1, 10, 14, 30, 0).unwrap(),
            raw_entry_price: entry,
            entry_price: entry,
            shares,
            stop_loss: entry - 5.0,
            take_profit: entry + 10.0,
            entry_cost: entry * shares.to_f64(),
            entry_commission: 1.0,
            entry_slippage: 0.01,
            entry_atr: 2.0,
            highest_put_wall: 0.0,
            spike_bar: 0,
            max_high: entry,
            highest_close: entry,
            hurst_exhaust_bars: 0,
            bars_held: 0,
        }
    }

    fn make_bar(close: f64) -> OhlcBar {
        OhlcBar {
            timestamp: chrono::Utc.with_ymd_and_hms(2025, 1, 10, 15, 0, 0).unwrap(),
            open: close, high: close, low: close, close, volume: 100.0,
        }
    }

    #[test]
    fn mark_to_market_no_position() {
        let cfg = StrategyConfig::default();
        let ts = TickerState::new(&cfg);
        assert!((ts.mark_to_market()).abs() < 0.01);
    }

    #[test]
    fn mark_to_market_uses_last_bar() {
        let cfg = StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);
        ts.position = Some(make_pos(10, 100.0));
        ts.last_bar = Some(make_bar(105.0));
        assert!((ts.mark_to_market() - 1050.0).abs() < 0.01);
    }

    #[test]
    fn mark_to_market_no_bar_uses_entry() {
        let cfg = StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);
        ts.position = Some(make_pos(10, 100.0));
        assert!((ts.mark_to_market() - 1000.0).abs() < 0.01);
    }

    #[test]
    fn record_trade_updates_pnl_and_peak() {
        let cfg = StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);
        let trade = Trade {
            ticker: crate::config::Ticker::AAPL,
            signal: Signal::LongVannaFlip,
            entry_time: "2025-01-10T14:30:00Z".into(),
            exit_time: "2025-01-10T15:00:00Z".into(),
            entry_price: 100.0, exit_price: 105.0,
            shares: 10, gross_pnl: 50.0, net_pnl: 48.0,
            return_pct: 5.0, commission: 1.0, slippage: 1.0,
            exit_reason: "tp".into(), bars_held: 5, spike_bar: 0,
            max_runup_atr: 3.0, diagnostics: None,
        };
        ts.record_trade(RecordTradeInputs { trade, bar_time_sec: 100, starting_capital: 10_000.0 });
        assert!((ts.cumulative_pnl - 48.0).abs() < 0.01);
        assert_eq!(ts.trades.len(), 1);
    }

    #[test]
    fn record_trade_drawdown() {
        let cfg = StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);

        let t1 = Trade {
            ticker: crate::config::Ticker::AAPL,
            signal: Signal::LongVannaFlip,
            entry_time: "t".into(), exit_time: "t".into(),
            entry_price: 100.0, exit_price: 110.0,
            shares: 10, gross_pnl: 100.0, net_pnl: 100.0,
            return_pct: 10.0, commission: 0.0, slippage: 0.0,
            exit_reason: "tp".into(), bars_held: 5, spike_bar: 0,
            max_runup_atr: 5.0, diagnostics: None,
        };
        ts.record_trade(RecordTradeInputs { trade: t1, bar_time_sec: 100, starting_capital: 10_000.0 });
        assert!((ts.peak_pnl - 100.0).abs() < 0.01);

        let t2 = Trade {
            ticker: crate::config::Ticker::AAPL,
            signal: Signal::LongVannaFlip,
            entry_time: "t".into(), exit_time: "t".into(),
            entry_price: 100.0, exit_price: 95.0,
            shares: 10, gross_pnl: -50.0, net_pnl: -50.0,
            return_pct: -5.0, commission: 0.0, slippage: 0.0,
            exit_reason: "sl".into(), bars_held: 3, spike_bar: 0,
            max_runup_atr: 1.0, diagnostics: None,
        };
        ts.record_trade(RecordTradeInputs { trade: t2, bar_time_sec: 200, starting_capital: 10_000.0 });
        assert!((ts.max_drawdown - 50.0).abs() < 0.01);
    }

    #[test]
    fn reset_daily_clears() {
        let cfg = StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);
        ts.daily.entries = 3;
        ts.reset_daily();
        assert_eq!(ts.daily.entries, 0);
    }
}
