use crate::config::strategy::{IV_BASELINE_EMA_DAYS, WB_ZONE_RESET_PCT};
use crate::config::{BarIndex, StrategyConfig};
use crate::strategy::indicators::IndicatorValues;
use crate::strategy::wall_smoother::{SmoothedWallLevels, WallSmoother, WallSmootherGexBundle};
use crate::strategy::zone::{WallTrackParams, ZoneState, ZoneTickBar};
use crate::types::{GexProfile, OhlcBar, Signal, SignalReason, TradeSignal, ToF64};
use chrono::Datelike;

use super::entries::iv_eligibility::SpikeApplyMut;
use super::entries::{BarCtx, SpreadBandInputs};

/// Raw wall vs smoothed reference + ATR — for spread / drift in ATR units (VF/WB + GEX pipeline).
#[derive(Debug, Clone, Copy)]
pub struct WallDiffInputs {
    pub wall: f64,
    pub smooth: f64,
    pub atr: f64,
}

impl WallDiffInputs {
    #[inline]
    pub const fn new(wall: f64, smooth: f64, atr: f64) -> Self {
        Self { wall, smooth, atr }
    }

    /// `(wall − smooth) / atr` when `wall` and `atr` are valid; else `0`.
    #[inline]
    pub fn diff_atr(self) -> f64 {
        if self.wall > 0.0 && self.atr > 0.0 {
            (self.wall - self.smooth) / self.atr
        } else {
            0.0
        }
    }

    /// WallBounce band: (trail CW − narrow PW) / ATR, or `0` when the band is not a valid spread.
    #[inline]
    pub fn wb_trail_band_spread_atr(pw: f64, cw: f64, atr: f64) -> f64 {
        if pw > 0.0 && cw > pw && atr > 0.0 {
            Self::new(cw, pw, atr).diff_atr()
        } else {
            0.0
        }
    }
}

/// Narrow/spread wall distances vs smoothed references in ATR units (VF gates, chart tooltips, IV scan).
#[derive(Debug, Clone, Copy, Default)]
pub struct WallAtrDiffs {
    /// (narrow_pw − smoothed_pw) / ATR
    pub pw_spw_atr: f64,
    /// (narrow_cw − spread_cw) / ATR
    pub cw_scw_atr: f64,
    /// (spread_cw − spread_pw) / ATR
    pub spread_atr: f64,
}

impl WallAtrDiffs {
    /// Recompute from narrow GEX walls + smoothed / EMA spread levels (after [`SignalState::smoothed_walls`] update).
    #[inline]
    pub fn from_narrow_and_smoothed(npw: f64, ncw: f64, s: &SignalState, ind: &IndicatorValues) -> Self {
        Self {
            pw_spw_atr: WallDiffInputs::new(npw, s.smoothed_put_wall(), ind.atr).diff_atr(),
            cw_scw_atr: WallDiffInputs::new(ncw, s.spread_call_wall(), ind.atr).diff_atr(),
            spread_atr: WallDiffInputs::new(s.spread_call_wall(), s.spread_put_wall(), ind.atr).diff_atr(),
        }
    }

    /// Optional CW/PW gaps for [`crate::strategy::entries::vf_gates::VfGateCtx`] when narrow walls exist (IV scan).
    #[inline]
    pub fn vf_gate_wall_opts(&self, npw: f64, ncw: f64, spw: f64, spread_cw: f64) -> (Option<f64>, Option<f64>) {
        let cw_vs = if ncw > 0.0 && spread_cw > 0.0 {
            Some(self.cw_scw_atr)
        } else {
            None
        };
        let pw_vs = if npw > 0.0 && spw > 0.0 {
            Some(self.pw_spw_atr)
        } else {
            None
        };
        (cw_vs, pw_vs)
    }
}

/// Max distance from PW in ATR for WB zone tracking. Structural constant.
pub const WB_MAX_PW_DIST_ATR: f64 = 2.0;
/// Min OTM % above spot for WB call wall selection (filters ATM noise).
pub const WB_CW_MIN_OTM_PCT: f64 = 0.03;

pub struct SignalState {
    pub bar_index: BarIndex,
    pub holding: Signal,
    pub entry_bar: BarIndex,
    pub entry_price: f64,
    /// Zone dwell tracker for WB (calm-path put wall proximity).
    pub entry_pw_zone: ZoneState,
    /// ATM put IV at the spike peak (when IV > baseline × iv_spike_mult).
    /// 0.0 = no spike detected yet.
    pub iv_spike_level: f64,
    /// Bar index at which the IV spike was detected. -1 = no spike.
    pub iv_spike_bar: BarIndex,
    /// EMA of atm_put_iv (slow baseline). Updated every bar where atm_put_iv is available.
    pub iv_baseline_ema: f64,
    /// The previous EOD close price. Used for verbose logging.
    pub prev_eod_close: f64,
    /// Highest atm_put_iv seen today (reset at start of each new trading day).
    /// Using daily max captures intraday panic peaks that already compress by market close.
    pub iv_daily_high: f64,
    /// The date (day-of-year, 1-based) of the last bar processed, used to detect day boundaries.
    pub last_bar_day: u32,
    /// Narrow + spread smoothed strikes and γ concentrations (from [`WallSmoother`] each GEX bar).
    pub smoothed_walls: SmoothedWallLevels,
    /// Consecutive bars where narrow CW < smoothed CW. Tracks structural CW weakness.
    pub cw_below_smooth_bars: u32,
    /// True while spike conditions are active (IV above threshold + below PW).
    /// Cleared when conditions reset, so a new episode can trigger a fresh spike.
    pub spike_episode_active: bool,

    // ── Spike-consumption accumulators (reset on each new spike) ──
    /// Close price at the spike detection bar.
    pub spike_close: f64,
    /// Previous bar's IV (for computing bar-to-bar IV changes).
    pub spike_prev_iv: f64,
    /// Cumulative IV drop since spike: Σ max(0, iv_{i-1} - iv_i).
    pub spike_cum_iv_drop: f64,
    /// Cumulative net price drift in ATR units since spike.
    pub spike_cum_return_atr: f64,
    /// Max favorable excursion from spike close in ATR units.
    pub spike_mfe_atr: f64,
    /// Max adverse excursion from spike close in ATR units.
    pub spike_mae_atr: f64,
    /// Previous bar's close (for computing bar-to-bar returns).
    pub spike_prev_close: f64,
    /// ATR at the spike detection bar.
    pub spike_atr: f64,
    /// Net vanna at the spike detection bar (positive = IV drop → dealer buy).
    pub spike_net_vanna: f64,
    /// Gamma tilt at the spike detection bar (positive = call-dominant, dealers dampen).
    pub spike_gamma_tilt: f64,
    /// Smoothed put wall at the spike detection bar (for pw drift since spike).
    pub spike_smoothed_pw: f64,
    /// EMA of |net_gex| for normalization (200-bar).
    pub gex_abs_ema: f64,
    /// Wall-vs-smoothed / spread distances in ATR (updated every GEX bar).
    pub wall_diff_atr: WallAtrDiffs,

    /// Fast EMA of atm_put_iv (5-bar ≈ 75 min on 15-min bars).
    pub iv_ema_fast: f64,
    /// Slow EMA of atm_put_iv (15-bar ≈ 225 min on 15-min bars).
    pub iv_ema_slow: f64,
    /// +1 = fast above slow (IV expanding), -1 = fast below slow (compressing), 0 = uninit.
    pub iv_cross_dir: i8,
}

impl SignalState {
    /// True when an IV spike has been recorded and not yet expired or cleared.
    pub fn has_active_spike(&self) -> bool {
        self.iv_spike_bar >= 0 && self.iv_spike_level > 0.0
    }

    /// Apply spike detection result to this state.
    pub fn apply_spike(&mut self, sc: &super::entries::iv_eligibility::SpikeConditions) {
        sc.apply(
            &mut SpikeApplyMut {
                level: &mut self.iv_spike_level,
                spike_bar: &mut self.iv_spike_bar,
                episode_active: &mut self.spike_episode_active,
            },
            self.bar_index,
        );
    }

    pub fn is_smoothed(&self) -> bool {
        self.smoothed_put_wall() > 0.0
    }

    /// γ-weighted smoothed narrow put wall (spike gate, wall trail, zone logic).
    #[inline]
    pub fn smoothed_put_wall(&self) -> f64 {
        self.smoothed_walls.smoothed_pw
    }

    /// γ-weighted smoothed narrow call wall.
    #[inline]
    pub fn smoothed_call_wall(&self) -> f64 {
        self.smoothed_walls.smoothed_cw
    }

    /// Pure-EMA spread put wall (VF spread gate).
    #[inline]
    pub fn spread_put_wall(&self) -> f64 {
        self.smoothed_walls.spread_pw
    }

    /// Pure-EMA spread call wall (VF spread gate).
    #[inline]
    pub fn spread_call_wall(&self) -> f64 {
        self.smoothed_walls.spread_cw
    }

    /// `(narrow_pw − smoothed_pw) / ATR` — VF gates, IV scan, chart.
    #[inline]
    pub fn pw_spw_atr(&self) -> f64 {
        self.wall_diff_atr.pw_spw_atr
    }

    /// `(narrow_cw − spread_cw) / ATR` — VF gates, IV scan, chart.
    #[inline]
    pub fn cw_scw_atr(&self) -> f64 {
        self.wall_diff_atr.cw_scw_atr
    }

    /// `(spread_cw − spread_pw) / ATR` — VF spread gate vs `vf_max_wall_spread_atr`.
    #[inline]
    pub fn spread_atr(&self) -> f64 {
        self.wall_diff_atr.spread_atr
    }

    /// For [`crate::strategy::entries::vf_gates::VfGateCtx`] / IV scan: optional wall-vs-smooth ATR gaps when narrow walls are active.
    #[inline]
    pub fn vf_gate_wall_atr_opts(&self, npw: f64, ncw: f64, spw: f64) -> (Option<f64>, Option<f64>) {
        self.wall_diff_atr.vf_gate_wall_opts(npw, ncw, spw, self.spread_call_wall())
    }

    #[inline]
    pub fn wall_diff_atr_ref(&self) -> &WallAtrDiffs {
        &self.wall_diff_atr
    }

    /// Normalized GEX: net_gex / EMA(|net_gex|). 0 if EMA too small.
    #[inline]
    pub fn gex_norm(&self, net_gex: f64) -> f64 {
        if self.gex_abs_ema > 1.0 { net_gex / self.gex_abs_ema } else { 0.0 }
    }

    /// IV compression as percentage of spike level (100% = no compression).
    #[inline]
    pub fn compress_pct(&self, iv: f64) -> f64 {
        if self.iv_spike_level > 0.0 { iv / self.iv_spike_level * 100.0 } else { 0.0 }
    }

    /// IV compression ratio (raw, not ×100).
    #[inline]
    pub fn compress_ratio(&self, iv: f64) -> f64 {
        if self.iv_spike_level > 0.0 { iv / self.iv_spike_level } else { 1.0 }
    }

    /// Bars elapsed since spike.
    #[inline]
    pub fn bars_since_spike(&self) -> BarIndex {
        self.bar_index - self.iv_spike_bar
    }

    /// Both smoothed walls are valid and ATR is positive.
    #[inline]
    pub fn has_valid_walls(&self, atr: f64) -> bool {
        self.smoothed_put_wall() > 0.0 && self.smoothed_call_wall() > 0.0 && atr > 0.0
    }

    /// Spread walls are valid (pw > 0, cw > pw, atr > 0).
    #[inline]
    pub fn has_valid_spread(&self, atr: f64) -> bool {
        let spw = self.spread_put_wall();
        self.spread_call_wall() > spw && spw > 0.0 && atr > 0.0
    }

    /// After [`WallSmoother::update_from_gex`]: copy smoothed levels + recompute wall-vs-ATR diffs.
    pub fn ingest_wall_smoother_gex_bar(
        &mut self,
        gex: &GexProfile,
        ind: &IndicatorValues,
        smoother: &WallSmoother,
        bundle: &WallSmootherGexBundle,
    ) {
        self.smoothed_walls = SmoothedWallLevels::from_smoother(smoother, bundle);
        self.update_wall_atr_diffs(gex, ind);
    }

    /// Narrow-CW vs smoothed streak, and wall-vs-smooth ATR diffs (after [`Self::ingest_wall_smoother_gex_bar`] updates [`Self::smoothed_walls`]).
    fn update_wall_atr_diffs(&mut self, gex: &GexProfile, ind: &IndicatorValues) {
        let (ncw, npw) = gex.narrow_cw_pw();
        let scw = self.smoothed_call_wall();
        if ncw > 0.0 && scw > 0.0 && ncw < scw {
            self.cw_below_smooth_bars += 1;
        } else {
            self.cw_below_smooth_bars = 0;
        }
        self.wall_diff_atr = WallAtrDiffs::from_narrow_and_smoothed(npw, ncw, self, ind);
    }

    /// Reset accumulators when a new spike fires.
    pub fn reset_spike_accum(&mut self, close: f64, iv: f64, ind: &IndicatorValues, gex: &GexProfile) {
        self.spike_close = close;
        self.spike_prev_close = close;
        self.spike_prev_iv = iv;
        self.spike_cum_iv_drop = 0.0;
        self.spike_cum_return_atr = 0.0;
        self.spike_mfe_atr = 0.0;
        self.spike_mae_atr = 0.0;
        self.spike_atr = ind.atr;
        self.spike_net_vanna = gex.net_vanna;
        self.spike_gamma_tilt = gex.gamma_tilt;
        self.spike_smoothed_pw = self.smoothed_put_wall();
    }

    /// Put wall drift since spike in ATR units. Positive = PW rising (bullish).
    #[inline]
    pub fn pw_drift_atr(&self, atr: f64) -> f64 {
        if self.spike_smoothed_pw > 0.0 && atr > 0.0 {
            (self.smoothed_put_wall() - self.spike_smoothed_pw) / atr
        } else {
            0.0
        }
    }

    /// Tick spike accumulators for the current bar.
    pub fn tick_spike_accum(&mut self, bar: &OhlcBar, ind: &IndicatorValues, gex: &GexProfile) {
        if !self.has_active_spike() || ind.atr <= 0.0 { return; }
        let iv = gex.atm_put_iv_or_zero();

        if self.spike_prev_iv > 0.0 && iv > 0.0 {
            let drop = (self.spike_prev_iv - iv).max(0.0);
            self.spike_cum_iv_drop += drop;
        }
        if iv > 0.0 { self.spike_prev_iv = iv; }

        if self.spike_prev_close > 0.0 {
            self.spike_cum_return_atr += (bar.close - self.spike_prev_close) / ind.atr;
        }
        self.spike_prev_close = bar.close;

        if self.spike_close > 0.0 {
            let up = (bar.high - self.spike_close) / ind.atr;
            let down = (self.spike_close - bar.low) / ind.atr;
            if up > self.spike_mfe_atr { self.spike_mfe_atr = up; }
            if down > self.spike_mae_atr { self.spike_mae_atr = down; }
        }
    }
}

impl Default for SignalState {
    fn default() -> Self {
        Self {
            bar_index: 0,
            holding: Signal::Flat,
            entry_bar: 0,
            entry_price: 0.0,
            entry_pw_zone: ZoneState::default(),
            iv_spike_level: 0.0,
            iv_spike_bar: -1,
            iv_baseline_ema: 0.0,
            prev_eod_close: 0.0,
            iv_daily_high: 0.0,
            last_bar_day: 0,
            smoothed_walls: SmoothedWallLevels::default(),
            cw_below_smooth_bars: 0,
            spike_episode_active: false,
            spike_close: 0.0,
            spike_prev_iv: 0.0,
            spike_cum_iv_drop: 0.0,
            spike_cum_return_atr: 0.0,
            spike_mfe_atr: 0.0,
            spike_mae_atr: 0.0,
            spike_prev_close: 0.0,
            spike_atr: 0.0,
            spike_net_vanna: 0.0,
            spike_gamma_tilt: 0.0,
            spike_smoothed_pw: 0.0,
            gex_abs_ema: 0.0,
            wall_diff_atr: WallAtrDiffs::default(),
            iv_ema_fast: 0.0,
            iv_ema_slow: 0.0,
            iv_cross_dir: 0,
        }
    }
}


impl SignalState {
    /// Core signal generator — LongVannaFlip (spike path) only.
    ///
    /// **Exit:** SL/TP + wall trailing via runner.
    pub fn generate_signal(
        &mut self,
        bar: &OhlcBar,
        gex: &GexProfile,
        indicators: &IndicatorValues,
        config: &StrategyConfig,
        ticker: crate::config::Ticker,
        verbose: bool,
    ) -> TradeSignal {
        self.bar_index += 1;

        // ── GEX normalization EMA ──────────────────────────────────────────
        {
            const GEX_EMA_ALPHA: f64 = 2.0 / 501.0; // 500-bar EMA (~3 trading days)
            let abs_gex = gex.net_gex.abs();
            if self.gex_abs_ema == 0.0 {
                self.gex_abs_ema = abs_gex.max(1.0);
            } else {
                self.gex_abs_ema = GEX_EMA_ALPHA * abs_gex + (1.0 - GEX_EMA_ALPHA) * self.gex_abs_ema;
            }
        }

        // ── IV tracking + spike detection ──────────────────────────────────
        let is_eod = bar.is_eod();

        let bar_day = bar.timestamp.ordinal();
        if bar_day != self.last_bar_day {
            self.iv_daily_high = 0.0;
            self.last_bar_day = bar_day;
        }
        if let Some(iv) = gex.atm_put_iv {
            if iv > self.iv_daily_high {
                self.iv_daily_high = iv;
            }
        }

        // ── IV fast/slow EMA (VIX Reversal-style crossover) ──────────────
        if let Some(iv) = gex.atm_put_iv {
            if iv > 0.0 {
                const IV_FAST_ALPHA: f64 = 2.0 / 261.0;  // 260-bar ≈ 10 trading days
                const IV_SLOW_ALPHA: f64 = 2.0 / 781.0;  // 780-bar ≈ 30 trading days
                if self.iv_ema_fast == 0.0 {
                    self.iv_ema_fast = iv;
                    self.iv_ema_slow = iv;
                } else {
                    self.iv_ema_fast = IV_FAST_ALPHA * iv + (1.0 - IV_FAST_ALPHA) * self.iv_ema_fast;
                    self.iv_ema_slow = IV_SLOW_ALPHA * iv + (1.0 - IV_SLOW_ALPHA) * self.iv_ema_slow;
                }
                let pct_diff = (self.iv_ema_fast - self.iv_ema_slow) / self.iv_ema_slow;
                let new_dir: i8 = if pct_diff > 0.05 { 1 } else if pct_diff < -0.05 { -1 } else { self.iv_cross_dir };
                self.iv_cross_dir = new_dir;
            }
        }

        // ── Intraday spike detection ─────────────────────────────────────
        let prev_spike_bar = self.iv_spike_bar;
        if let Some(sc) = super::entries::iv_eligibility::SpikeCheckInputs::from_signal_state(
            self, bar, indicators,
        ).check(config)
        {
            self.apply_spike(&sc);
        }
        if self.iv_spike_bar != prev_spike_bar && self.has_active_spike() {
            let iv = gex.atm_put_iv_or_zero();
            self.reset_spike_accum(bar.close, iv, indicators, gex);
        }

        self.tick_spike_accum(bar, indicators, gex);

        // ── EOD: baseline EMA + rolling high (kept clean from intraday noise) ──
        if is_eod {
            if let Some(iv) = gex.atm_put_iv {
                if iv > 0.0 {
                    let n = IV_BASELINE_EMA_DAYS.max(2).to_f64();
                    let alpha = 2.0 / (n + 1.0);
                    let iv_peak = self.iv_daily_high.max(iv);
                    let eff_mult = config.eff_iv_spike_mult(indicators.atr_regime_ratio);
                    if self.iv_baseline_ema <= 0.0 {
                        self.iv_baseline_ema = iv;
                    } else {
                        let spike_blocks_baseline = iv_peak > self.iv_baseline_ema * eff_mult;
                        if !spike_blocks_baseline {
                            self.iv_baseline_ema = alpha * iv + (1.0 - alpha) * self.iv_baseline_ema;
                        }
                    }
                    if verbose {
                        use super::entries::iv_eligibility::SpotWallAtrRegime;
                        let is_spiking = iv_peak > self.iv_baseline_ema * eff_mult;
                        let sw = SpotWallAtrRegime::new(
                            self.smoothed_put_wall(),
                            indicators.bar_vol_regime(bar.close),
                        );
                        let near_pw = sw.near_put_wall();
                        eprintln!(
                            "  [eod] {} c={:.2} spw={:.1} prev_c={:.2} | near_pw={} spike={} | iv_peak={:.3} iv_eod={:.3} base={:.3}×{:.1}={:.3}",
                            bar.timestamp.format(crate::types::DATE_FMT), bar.close, self.smoothed_put_wall(),
                            self.prev_eod_close,
                            near_pw, is_spiking,
                            iv_peak, iv, self.iv_baseline_ema, eff_mult,
                            self.iv_baseline_ema * eff_mult,
                        );
                    }
                }
            }

            self.prev_eod_close = bar.close;
        }

        if self.has_active_spike() && self.holding.is_flat() {
            let bars_since_spike = self.bar_index - self.iv_spike_bar;
            if bars_since_spike > config.eff_iv_lookback_bars() {
                self.iv_spike_level = 0.0;
                self.iv_spike_bar = -1;
            }
        }

        // ── WB zone tracking ──
        if ticker.is_wb_enabled() {
            let (pw, cw) = gex.wb_wall_pair(WB_CW_MIN_OTM_PCT);
            let width = indicators.atr * WB_MAX_PW_DIST_ATR;
            if pw > 0.0 {
                self.entry_pw_zone.track_wall(WallTrackParams { wall: pw, width, reset_pct: WB_ZONE_RESET_PCT });
            }
            let spread_atr = WallDiffInputs::wb_trail_band_spread_atr(pw, cw, indicators.atr);
            let spread_ok = SpreadBandInputs::spread_in_band(
                spread_atr,
                config.wb_min_wall_spread_atr,
                config.wb_max_wall_spread_atr(),
            );
            if spread_ok {
                self.entry_pw_zone.tick(&ZoneTickBar {
                    close: bar.close,
                    low: bar.low,
                    high: bar.high,
                    atr: indicators.atr,
                });
            }
            if pw > 0.0 && indicators.atr > 0.0 {
                let anchor = self.entry_pw_zone.anchor_level;
                if anchor > 0.0 && bar.low < anchor {
                    let depth = (anchor - bar.low) / indicators.atr;
                    if self.entry_pw_zone.pierce_bars_ago <= 0 {
                        self.entry_pw_zone.max_pierce_depth = depth;
                    } else {
                        self.entry_pw_zone.max_pierce_depth = self.entry_pw_zone.max_pierce_depth.max(depth);
                    }
                    self.entry_pw_zone.pierce_bars_ago = 0;
                } else if self.entry_pw_zone.pierce_bars_ago >= 0 {
                    self.entry_pw_zone.pierce_bars_ago += 1;
                }
            }
        }

        let mk_hold = |signal: Signal| TradeSignal {
            timestamp: bar.timestamp,
            signal,
            price: bar.close,
            reason: SignalReason::Hold,
        };

        if !self.holding.is_flat() {
            if let Some(exit_signal) = self.check_exit() {
                return exit_signal;
            }
            return mk_hold(self.holding);
        }

        let bctx = BarCtx::new(self, bar, gex, indicators, config, ticker);
        if let Some(entry_signal) = bctx.check_entry() {
            return entry_signal;
        }

        let reason = bctx.flat_reason();
        TradeSignal {
            timestamp: bar.timestamp,
            signal: Signal::Flat,
            price: bar.close,
            reason: SignalReason::Flat(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WallLevel;

    fn test_config() -> StrategyConfig {
        StrategyConfig::default()
    }

    fn bar(close: f64, low: f64, high: f64) -> OhlcBar {
        OhlcBar {
            timestamp: chrono::Utc::now(),
            open: close,
            high,
            low,
            close,
            volume: 10_000.0,
        }
    }

    fn gex(pw: f64, cw: f64, net: f64) -> GexProfile {
        GexProfile {
            spot: (pw + cw) / 2.0,
            net_gex: net,
            put_walls: vec![WallLevel { strike: pw, gamma_oi: 1.0 }],
            call_walls: vec![WallLevel { strike: cw, gamma_oi: 1.0 }],
            atm_put_iv: Some(0.25),
            wide_put_walls: vec![WallLevel { strike: pw - 5.0, gamma_oi: 0.5 }],
            wide_call_walls: vec![WallLevel { strike: cw + 5.0, gamma_oi: 0.5 }],
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

    fn warm_indicators() -> IndicatorValues {
        IndicatorValues {
            atr: 1.5,
            ema_fast: 100.0,
            ema_slow: 100.0,
            adx: 15.0,
            trend_ema: 100.0,
            tsi: 5.0,
            tsi_bullish: true,
            atr_regime_ratio: 1.0,
            atr_regime_ema: 1.5,
        }
    }

    #[test]
    fn flat_when_no_walls() {
        let cfg = test_config();
        let ind = warm_indicators();
        let mut signal_state = SignalState::default();
        let g = GexProfile::empty(100.0);
        let b = bar(100.0, 99.5, 100.5);
        let sig = signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert!(sig.signal.is_flat());
    }

    #[test]
    fn does_not_modify_holding_on_exit() {
        let cfg = test_config();
        let ind = warm_indicators();
        let mut signal_state = SignalState::default();
        signal_state.holding = Signal::LongVannaFlip;
        signal_state.entry_price = 100.0;
        let g = gex(99.0, 102.0, 100.0);
        let b = bar(102.5, 101.0, 103.0);
        let _ = signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert_eq!(signal_state.holding, Signal::LongVannaFlip);
    }

    #[test]
    fn holding_returns_hold_signal() {
        let cfg = test_config();
        let ind = warm_indicators();
        let mut signal_state = SignalState::default();
        signal_state.holding = Signal::LongVannaFlip;
        signal_state.entry_price = 100.0;
        let g = gex(99.0, 102.0, 100.0);
        let b = bar(100.5, 100.0, 101.0);
        let sig = signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert_eq!(sig.signal, Signal::LongVannaFlip);
        assert!(matches!(sig.reason, SignalReason::Hold));
    }

    #[test]
    fn bar_index_increments() {
        let cfg = test_config();
        let ind = warm_indicators();
        let mut signal_state = SignalState::default();
        let g = GexProfile::empty(100.0);
        let b = bar(100.0, 99.5, 100.5);
        signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert_eq!(signal_state.bar_index, 1);
        signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert_eq!(signal_state.bar_index, 2);
    }

    #[test]
    fn signal_carries_price() {
        let cfg = test_config();
        let ind = warm_indicators();
        let mut signal_state = SignalState::default();
        let g = gex(99.0, 102.0, 100.0);
        let b = bar(100.0, 99.5, 100.5);
        let sig = signal_state.generate_signal(&b, &g, &ind, &cfg, crate::config::Ticker::AAPL, false);
        assert!((sig.price - 100.0).abs() < 0.01);
    }

    #[test]
    fn has_active_spike_false_by_default() {
        let signal_state = SignalState::default();
        assert!(!signal_state.has_active_spike());
    }

    #[test]
    fn has_active_spike_true_after_set() {
        let mut signal_state = SignalState::default();
        signal_state.iv_spike_level = 0.50;
        signal_state.iv_spike_bar = 5;
        assert!(signal_state.has_active_spike());
    }

    #[test]
    fn is_smoothed() {
        let mut signal_state = SignalState::default();
        assert!(!signal_state.is_smoothed());
        signal_state.smoothed_walls.smoothed_pw = 100.0;
        signal_state.smoothed_walls.smoothed_cw = 105.0;
        assert!(signal_state.is_smoothed());
    }
}
