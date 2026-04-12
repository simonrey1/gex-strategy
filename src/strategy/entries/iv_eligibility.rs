use crate::config::{BarIndex, StrategyConfig};
use crate::strategy::signals::SignalState;
use crate::types::{BarVolRegime, GexProfile, OhlcBar};
use crate::strategy::indicators::IndicatorValues;

/// Smoothed put wall + vol regime (vol.close = spot) — shared by spike checks and verbose logging.
#[derive(Debug, Clone, Copy)]
pub struct SpotWallAtrRegime {
    pub spw: f64,
    pub vol: BarVolRegime,
}

impl SpotWallAtrRegime {
    #[inline]
    pub const fn new(spw: f64, vol: BarVolRegime) -> Self {
        Self { spw, vol }
    }

    /// Whether spot is near/below the smoothed put wall (regime-adaptive slack).
    #[inline]
    pub fn near_put_wall(&self) -> bool {
        if self.spw <= 0.0 || self.vol.atr <= 0.0 {
            return false;
        }
        let dist = (self.vol.close - self.spw) / self.vol.atr;
        let regime_slack = (1.0 / self.vol.atr_regime_ratio.max(0.3) - 1.0).max(0.0);
        dist <= regime_slack
    }
}

/// Raw IV / spot / vol / wall inputs for [`SpikeCheckInputs::check`].
#[derive(Debug, Clone, Copy)]
pub struct SpikeCheckInputs {
    pub iv_baseline_ema: f64,
    pub iv_daily_high: f64,
    pub spot_wall: SpotWallAtrRegime,
    pub tsi: f64,
    /// Smoothed call wall (γ-weighted narrow).
    pub scw: f64,
    /// (spread_cw − spread_pw) / ATR — wall corridor width.
    pub wall_spread_atr: f64,
    /// (spot − scw) / ATR — call wall proximity (negative = below CW).
    pub cw_dist_atr: f64,
}

impl SpikeCheckInputs {
    /// Build from raw [`SignalState`] + bar + indicators (shared by live signal gen and IV scan).
    #[inline]
    pub fn from_signal_state(ss: &SignalState, bar: &OhlcBar, ind: &IndicatorValues) -> Self {
        let vol = ind.bar_vol_regime(bar.close);
        let scw = ss.smoothed_call_wall();
        let atr = ind.atr;
        let spread_atr = if atr > 0.0 && ss.spread_call_wall() > 0.0 && ss.spread_put_wall() > 0.0 {
            (ss.spread_call_wall() - ss.spread_put_wall()) / atr
        } else { 0.0 };
        let cw_dist = if atr > 0.0 && scw > 0.0 {
            (bar.close - scw) / atr
        } else { 0.0 };
        Self {
            iv_baseline_ema: ss.iv_baseline_ema,
            iv_daily_high: ss.iv_daily_high,
            spot_wall: SpotWallAtrRegime::new(ss.smoothed_put_wall(), vol),
            tsi: ind.tsi,
            scw,
            wall_spread_atr: spread_atr,
            cw_dist_atr: cw_dist,
        }
    }

    /// Evaluate whether this bar qualifies as a spike event. Pure — no state mutation.
    pub fn check(&self, config: &StrategyConfig) -> Option<SpikeConditions> {
        if self.iv_baseline_ema <= 0.0 {
            return None;
        }
        let sw = self.spot_wall;
        let is_spiking = self.iv_daily_high
            > self.iv_baseline_ema * config.eff_iv_spike_mult(sw.vol.atr_regime_ratio);
        let near_pw = sw.near_put_wall();
        let atr_ok = config.spike_min_atr_pct <= 0.0
            || (sw.vol.close > 0.0 && sw.vol.atr / sw.vol.close * 100.0 >= config.spike_min_atr_pct);
        let tsi_max_ok = config.spike_max_tsi <= 0.0 || self.tsi <= config.spike_max_tsi;
        let tsi_min_ok = config.spike_min_tsi >= 0.0 || self.tsi >= config.spike_min_tsi;
        let spread_ok = config.spike_min_wall_spread_atr <= 0.0
            || self.wall_spread_atr >= config.spike_min_wall_spread_atr;
        let spread_max_ok = config.spike_max_wall_spread_atr <= 0.0
            || self.wall_spread_atr <= config.spike_max_wall_spread_atr;
        let cw_ok = config.spike_max_cw_dist_atr <= 0.0
            || self.cw_dist_atr <= config.spike_max_cw_dist_atr;
        let conditions_met = is_spiking && near_pw && atr_ok
            && tsi_max_ok && tsi_min_ok
            && spread_ok && spread_max_ok && cw_ok;
        Some(SpikeConditions { conditions_met, iv_peak: self.iv_daily_high })
    }
}

impl<'a> From<&'a super::BarCtx<'a>> for SpikeCheckInputs {
    #[inline]
    fn from(bctx: &'a super::BarCtx<'a>) -> Self {
        Self::from_signal_state(bctx.state, bctx.bar, bctx.ind)
    }
}

/// Result of evaluating spike conditions on a single bar.
pub struct SpikeConditions {
    pub conditions_met: bool,
    pub iv_peak: f64,
}

/// Mutable spike episode fields for [`SpikeConditions::apply`].
pub struct SpikeApplyMut<'a> {
    pub level: &'a mut f64,
    pub spike_bar: &'a mut BarIndex,
    pub episode_active: &'a mut bool,
}

impl SpikeConditions {
    /// Apply to any spike state bundle. Both `SignalState` and `ScanSpikeState`
    /// delegate here so the logic lives in one place.
    pub fn apply(&self, m: &mut SpikeApplyMut<'_>, bar_index: BarIndex) {
        if !self.conditions_met {
            *m.episode_active = false;
        } else if !*m.episode_active {
            *m.level = self.iv_peak;
            *m.spike_bar = bar_index;
            *m.episode_active = true;
        }
    }
}

/// Spike + lookback + ATM IV + TSI for [`IvCompressionInputs::is_eligible`].
#[derive(Debug, Clone, Copy)]
pub struct IvCompressionInputs {
    pub has_spike: bool,
    pub bars_since_spike: BarIndex,
    pub atm_put_iv: Option<f64>,
    pub max_bars: BarIndex,
    pub tsi: f64,
    pub elig_tsi_oversold: f64,
    pub elig_early_bars: BarIndex,
}

impl IvCompressionInputs {
    /// Whether IV compression conditions allow opening a scan / entering a trade.
    ///
    /// Tree-derived rule: when TSI is not oversold, only early entries
    /// tend to be profitable. Late non-oversold entries are predominantly
    /// true-worst in scan analysis.
    #[inline]
    pub fn is_eligible(self) -> bool {
        self.has_spike
            && self.bars_since_spike <= self.max_bars
            && matches!(self.atm_put_iv, Some(v) if v > 0.0)
            && (self.elig_early_bars <= 0
                || self.tsi <= self.elig_tsi_oversold
                || self.bars_since_spike <= self.elig_early_bars)
    }
}

impl SignalState {
    /// Whether IV compression conditions allow opening a scan / entering a trade.
    pub fn iv_scan_eligible(&self, gex: &GexProfile, cfg: &StrategyConfig, tsi: f64) -> bool {
        IvCompressionInputs {
            has_spike: self.has_active_spike(),
            bars_since_spike: self.bar_index - self.iv_spike_bar,
            atm_put_iv: gex.atm_put_iv,
            max_bars: cfg.eff_iv_lookback_bars(),
            tsi,
            elig_tsi_oversold: cfg.elig_tsi_oversold,
            elig_early_bars: cfg.elig_early_bars as BarIndex,
        }
        .is_eligible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> StrategyConfig {
        StrategyConfig::default()
    }

    fn spike_inp(baseline: f64, high: f64, spw: f64, spot: f64, atr: f64) -> SpikeCheckInputs {
        SpikeCheckInputs {
            iv_baseline_ema: baseline, iv_daily_high: high,
            spot_wall: SpotWallAtrRegime::new(spw, BarVolRegime::new(spot, atr, 1.0)),
            tsi: -10.0, scw: spot + 3.0 * atr, wall_spread_atr: 6.0, cw_dist_atr: -3.0,
        }
    }

    #[test]
    fn spike_conditions_none_when_baseline_zero() {
        let r = spike_inp(0.0, 0.5, 95.0, 100.0, 2.0).check(&default_cfg());
        assert!(r.is_none());
    }

    #[test]
    fn spike_conditions_not_met_low_iv() {
        let r = spike_inp(0.20, 0.21, 95.0, 100.0, 2.0).check(&default_cfg()).unwrap();
        assert!(!r.conditions_met);
    }

    #[test]
    fn spike_conditions_met_high_iv() {
        let mut cfg = default_cfg();
        cfg.iv_spike_mult = 1.2;
        cfg.spike_min_atr_pct = 0.0;
        let r = spike_inp(0.20, 0.30, 100.0, 100.0, 2.0).check(&cfg).unwrap();
        assert!(r.conditions_met);
    }

    #[test]
    fn apply_records_spike() {
        let sc = SpikeConditions { conditions_met: true, iv_peak: 0.35 };
        let (mut level, mut bar, mut episode) = (0.0, 0_i64, false);
        sc.apply(
            &mut SpikeApplyMut {
                level: &mut level,
                spike_bar: &mut bar,
                episode_active: &mut episode,
            },
            42,
        );
        assert!(episode);
        assert_eq!(bar, 42);
        assert!((level - 0.35).abs() < 1e-10);
    }

    #[test]
    fn apply_no_double_record() {
        let sc = SpikeConditions { conditions_met: true, iv_peak: 0.40 };
        let (mut level, mut bar, mut episode) = (0.35, 10, true);
        sc.apply(
            &mut SpikeApplyMut {
                level: &mut level,
                spike_bar: &mut bar,
                episode_active: &mut episode,
            },
            20,
        );
        assert_eq!(bar, 10, "should not overwrite existing episode");
    }

    #[test]
    fn apply_clears_on_not_met() {
        let sc = SpikeConditions { conditions_met: false, iv_peak: 0.0 };
        let (mut level, mut bar, mut episode) = (0.35, 10, true);
        sc.apply(
            &mut SpikeApplyMut {
                level: &mut level,
                spike_bar: &mut bar,
                episode_active: &mut episode,
            },
            20,
        );
        assert!(!episode);
    }

    fn elig(has_spike: bool, bars: BarIndex, iv: Option<f64>, tsi: f64) -> IvCompressionInputs {
        IvCompressionInputs {
            has_spike, bars_since_spike: bars, atm_put_iv: iv, max_bars: 50,
            tsi, elig_tsi_oversold: -5.0, elig_early_bars: 20,
        }
    }

    #[test]
    fn compression_eligible_basic() {
        assert!(elig(true, 1, Some(0.25), -10.0).is_eligible());
        assert!(!elig(false, 1, Some(0.25), -10.0).is_eligible());
    }

    #[test]
    fn compression_expired() {
        assert!(!elig(true, 51, Some(0.25), -10.0).is_eligible());
    }

    #[test]
    fn compression_no_iv() {
        assert!(!elig(true, 1, None, -10.0).is_eligible());
        assert!(!elig(true, 1, Some(0.0), -10.0).is_eligible());
    }

    #[test]
    fn compression_tsi_gate() {
        assert!(elig(true, 40, Some(0.25), -10.0).is_eligible());
        assert!(elig(true, 15, Some(0.25), 10.0).is_eligible());
        assert!(!elig(true, 25, Some(0.25), 10.0).is_eligible());
    }

    #[test]
    fn compression_tsi_gate_disabled() {
        let mut inp = elig(true, 40, Some(0.25), 10.0);
        inp.elig_early_bars = 0;
        assert!(inp.is_eligible());
    }
}
