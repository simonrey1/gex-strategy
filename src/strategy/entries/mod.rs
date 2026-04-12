mod guards;
pub use guards::{SmoothedWalls, SpreadBandInputs};
pub(crate) mod iv_eligibility;
pub(crate) mod vf_gates;
pub(crate) mod wall_bounce;

use self::vf_gates::RegimeCtx;

use crate::config::{StrategyConfig, Ticker};
use crate::strategy::config_stops::PlainStopInputs;
use crate::strategy::indicators::IndicatorValues;
use crate::strategy::signals::{SignalState, WallAtrDiffs, WB_CW_MIN_OTM_PCT};
use crate::types::{AtrRegimeTsi, EntryAtrTsi, GexProfile, OhlcBar, Rejection, Signal, SignalReason, TradeSignal};

/// Bar + strategy variant + entry reason string → [`TradeSignal`] (VF / WB entry paths).
pub(crate) struct EntryTradeSignalBuild<'a> {
    pub bar: &'a OhlcBar,
    pub signal: Signal,
    pub reason: String,
}

impl<'a> EntryTradeSignalBuild<'a> {
    #[inline]
    pub fn new(bar: &'a OhlcBar, signal: Signal, reason: String) -> Self {
        Self { bar, signal, reason }
    }

    #[inline]
    pub fn into_trade_signal(self) -> TradeSignal {
        TradeSignal {
            timestamp: self.bar.timestamp,
            signal: self.signal,
            price: self.bar.close,
            reason: SignalReason::Entry(self.reason),
        }
    }
}

/// Immutable context for signal evaluation on a single strategy bar.
pub struct BarCtx<'a> {
    pub state: &'a SignalState,
    pub bar: &'a OhlcBar,
    pub gex: &'a GexProfile,
    pub ind: &'a IndicatorValues,
    pub cfg: &'a StrategyConfig,
    pub ticker: Ticker,
}

impl<'a> BarCtx<'a> {
    pub fn new(
        state: &'a SignalState, bar: &'a OhlcBar, gex: &'a GexProfile,
        ind: &'a IndicatorValues, cfg: &'a StrategyConfig, ticker: Ticker,
    ) -> Self {
        Self { state, bar, gex, ind, cfg, ticker }
    }

    /// [`EntryAtrTsi`] from warmed indicators (same as [`IndicatorValues::entry_atr_tsi`] on `self.ind`).
    #[inline]
    pub fn entry_atr_tsi(&self) -> EntryAtrTsi {
        self.ind.entry_atr_tsi()
    }

    #[inline]
    pub fn regime_ctx(&self) -> RegimeCtx {
        RegimeCtx::from(self.ind)
    }

    /// VF gate ATR% pair from this bar's GEX + indicators.
    #[inline]
    pub fn vf_atr_pct_pair(&self) -> (f64, f64) {
        self.gex.vf_atr_pct_pair(self.ind.atr, self.ind.atr_regime_ema)
    }

    #[inline]
    pub fn net_gex(&self) -> f64 {
        self.gex.net_gex
    }

    #[inline]
    pub fn gex_spot(&self) -> f64 {
        self.gex.spot
    }

    #[inline]
    pub fn atm_put_iv_or_zero(&self) -> f64 {
        self.gex.atm_put_iv_or_zero()
    }

    #[inline]
    pub fn atm_gamma_dominance(&self) -> f64 {
        self.gex.atm_gamma_dominance
    }

    #[inline]
    pub fn narrow_pw(&self) -> f64 {
        self.gex.pw()
    }

    #[inline]
    pub fn narrow_cw(&self) -> f64 {
        self.gex.cw()
    }

    #[inline]
    pub fn atm_put_iv_opt(&self) -> Option<f64> {
        self.gex.atm_put_iv
    }

    /// WB path: strongest trail call wall above min OTM.
    #[inline]
    pub fn wb_trail_cw(&self) -> f64 {
        self.gex.strongest_wide_cw(WB_CW_MIN_OTM_PCT)
    }

    /// Narrow PW + WB trail CW (`GexProfile::wb_wall_pair` at min OTM).
    #[inline]
    pub fn wb_wall_pair(&self) -> (f64, f64) {
        self.gex.wb_wall_pair(WB_CW_MIN_OTM_PCT)
    }

    #[inline]
    pub fn bar_timestamp_sec(&self) -> i64 {
        self.bar.timestamp.timestamp()
    }

    #[inline] pub fn atr(&self) -> f64 { self.ind.atr }
    #[inline] pub fn adx(&self) -> f64 { self.ind.adx }
    #[inline] pub fn tsi(&self) -> f64 { self.ind.tsi }
    #[inline] pub fn tsi_bullish(&self) -> bool { self.ind.tsi_bullish }
    #[inline] pub fn trend_ema(&self) -> f64 { self.ind.trend_ema }
    #[inline] pub fn ema_fast(&self) -> f64 { self.ind.ema_fast }
    #[inline] pub fn ema_slow(&self) -> f64 { self.ind.ema_slow }

    /// Hypothetical entry at bar open (IV scan, etc.) — same ATR/regime/TSI as intrabar signal path.
    #[inline]
    pub fn plain_stop_inputs_at_bar_open(&self) -> PlainStopInputs {
        PlainStopInputs {
            entry_price: self.bar.open,
            regime: AtrRegimeTsi::from(self.ind),
        }
    }

    /// VF SL/TP at bar open (IV scan hypotheticals).
    #[inline]
    pub fn compute_stops_at_bar_open(&self) -> Option<(f64, f64)> {
        self.cfg.compute_stops(&self.plain_stop_inputs_at_bar_open())
    }

    /// [`WallAtrDiffs`] for this bar (VF gates, chart tooltips).
    #[inline]
    pub fn wall_diff_atr(&self) -> WallAtrDiffs {
        self.state.wall_diff_atr
    }

    /// Borrowed [`WallAtrDiffs`] for VF gates / IV scan.
    #[inline]
    pub fn wall_diff_atr_ref(&self) -> &WallAtrDiffs {
        self.state.wall_diff_atr_ref()
    }

    /// Check entry conditions for all signal types.
    /// Returns `Some(TradeSignal)` on entry, `None` otherwise.
    pub fn check_entry(&self) -> Option<TradeSignal> {
        if !self.state.holding.is_flat() { return None; }
        if let Ok(sig) = wall_bounce::try_vanna_flip(self) {
            return Some(sig);
        }
        if let Ok(sig) = wall_bounce::try_wall_bounce_calm(self) {
            return Some(sig);
        }
        None
    }

    /// Human-readable reason why no entry fired (for verbose backtest output).
    pub fn flat_reason(&self) -> String {
        let vf = match wall_bounce::try_vanna_flip(self) {
            Err(reason) => reason,
            Ok(_) => return "no_signal".into(),
        };
        let wb = match wall_bounce::try_wall_bounce_calm(self) {
            Err(reason) => reason,
            Ok(_) => return "no_signal".into(),
        };
        format!("{vf} | {wb}")
    }

    /// All VF rejection reasons without short-circuiting (for `--rejections`).
    pub fn rejection_reasons(&self) -> Vec<Rejection> {
        match wall_bounce::vf_ctx(self) {
            Err(reasons) => reasons,
            Ok(ctx) => ctx.evaluate(true).err().unwrap_or_default(),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WallLevel;

    fn bar_at(close: f64) -> OhlcBar {
        OhlcBar {
            timestamp: chrono::Utc::now(),
            open: close,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume: 10_000.0,
        }
    }

    fn gex_with_iv(pw: f64, cw: f64, iv: f64) -> GexProfile {
        GexProfile {
            spot: (pw + cw) / 2.0,
            net_gex: 1000.0,
            put_walls: vec![WallLevel { strike: pw, gamma_oi: 1.0 }],
            call_walls: vec![WallLevel { strike: cw, gamma_oi: 1.0 }],
            atm_put_iv: Some(iv),
            wide_put_walls: vec![WallLevel { strike: pw - 1.0, gamma_oi: 0.5 }],
            wide_call_walls: vec![WallLevel { strike: cw + 1.0, gamma_oi: 0.5 }],
            pw_com_dist_pct: 0.0,
            pw_near_far_ratio: 0.0,
            atm_gamma_dominance: 0.0,
            near_gamma_imbalance: 0.0,
            total_put_goi: 0.0,
            total_call_goi: 0.0,
            cw_depth_ratio: 0.0,
            gamma_tilt: 0.0,
            net_delta: 0.0,
            net_vanna: 0.0,
        }
    }

    fn test_indicators() -> IndicatorValues {
        IndicatorValues {
            atr: 0.35, ema_fast: 100.0, ema_slow: 99.0,
            adx: 25.0, trend_ema: 90.0, tsi: 30.0, tsi_bullish: true,
            atr_regime_ratio: 1.0, atr_regime_ema: 0.35,
        }
    }

    fn spike_config() -> crate::config::StrategyConfig {
        crate::config::StrategyConfig::default()
    }

    #[test]
    fn spike_path_fires_when_iv_compressed() {
        let s = SignalState {
            bar_index: 100,
            iv_spike_level: 0.50,
            iv_spike_bar: 85,
            iv_baseline_ema: 0.35,
            smoothed_walls: crate::strategy::wall_smoother::SmoothedWallLevels {
                smoothed_pw: 98.0,
                smoothed_cw: 101.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let gex = gex_with_iv(98.0, 101.0, 0.25);
        let cfg = spike_config();
        let ind = test_indicators();
        let bar = bar_at(99.0);
        let ctx = BarCtx::new(&s, &bar, &gex, &ind, &cfg, crate::config::Ticker::AAPL);
        let reason = ctx.flat_reason();
        let sig = ctx.check_entry();
        assert!(sig.is_some(), "rejected: {reason}");
        assert_eq!(sig.unwrap().signal, Signal::LongVannaFlip);
    }

    #[test]
    fn no_entry_when_holding() {
        let s = SignalState {
            holding: Signal::LongVannaFlip,
            ..Default::default()
        };
        let gex = gex_with_iv(98.0, 101.0, 0.25);
        let cfg = spike_config();
        let ind = test_indicators();
        let bar = bar_at(99.0);
        let sig = BarCtx::new(&s, &bar, &gex, &ind, &cfg, crate::config::Ticker::AAPL).check_entry();
        assert!(sig.is_none());
    }

    #[test]
    fn no_entry_without_spike() {
        let s = SignalState {
            smoothed_walls: crate::strategy::wall_smoother::SmoothedWallLevels {
                smoothed_pw: 98.0,
                smoothed_cw: 101.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let gex = gex_with_iv(98.0, 101.0, 0.25);
        let cfg = spike_config();
        let ind = test_indicators();
        let bar = bar_at(99.0);
        let sig = BarCtx::new(&s, &bar, &gex, &ind, &cfg, crate::config::Ticker::AAPL).check_entry();
        assert!(sig.is_none(), "should not enter without IV spike");
    }

    #[test]
    fn flat_reason_gives_details_when_no_entry() {
        let s = SignalState::default();
        let gex = gex_with_iv(98.0, 101.0, 0.25);
        let cfg = spike_config();
        let ind = test_indicators();
        let bar = bar_at(99.0);
        let reason = BarCtx::new(&s, &bar, &gex, &ind, &cfg, crate::config::Ticker::AAPL).flat_reason();
        assert!(!reason.is_empty());
        assert!(reason.contains('|'), "should have VF | WB reasons: {reason}");
    }

    #[test]
    fn iv_scan_eligible_requires_spike() {
        let s = SignalState::default();
        let gex = gex_with_iv(98.0, 101.0, 0.25);
        let cfg = StrategyConfig::default();
        assert!(!s.iv_scan_eligible(&gex, &cfg, -10.0));
    }
}
