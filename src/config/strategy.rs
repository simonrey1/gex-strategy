use serde::{Deserialize, Serialize};
use super::bar_counts::{BarIndex, IV_LOOKBACK_BARS_INDEX};

// ─── Cache-affecting constants (changing these invalidates cached data) ───────
pub use super::bar_interval::{Minutes, BAR_INTERVAL_MINUTES};
pub const OPTION_MAX_EXPIRY_DAYS: u32 = 60;
pub const OPTION_STRIKE_RANGE_PCT: f64 = 0.05;

/// Number of strike steps to fetch around spot for options data.
pub fn option_strike_steps() -> u32 {
    ((OPTION_STRIKE_RANGE_PCT * 250.0) / 2.5)
        .ceil()
        .clamp(10.0, 60.0) as u32
}



// ── Fixed indicator lengths (standard textbook values, never tuned) ───────
pub const ATR_LENGTH: usize = 14;
pub const EMA_FAST_LEN: usize = 12;
pub const EMA_SLOW_LEN: usize = 26;
pub const DI_LENGTH: usize = 14;
pub const IV_LOOKBACK_BARS: usize = 50;
pub const IV_BASELINE_EMA_DAYS: usize = 5;
pub const TSI_LONG_LENGTH: usize = 13;
pub const TSI_SHORT_LENGTH: usize = 7;
pub const TSI_SIGNAL_LENGTH: usize = 7;
pub const HURST_WINDOW: usize = 128;
pub const HURST_MIN_PERSIST_BARS: u32 = 5;
pub const WALL_SMOOTH_HALFLIFE: usize = 2;
pub const ATR_REGIME_EMA_LEN: usize = 250;
pub const WB_MAX_BARS_SINCE_ABOVE: i32 = 24;
pub const WB_ZONE_RESET_PCT: f64 = 0.03;

// ── Structural constants ─────────────────────────────────────────────────
pub const HALF: f64 = 0.5;
pub const TSI_RANGE: f64 = 100.0;

// ── Golden ratio (φ = 1.618) powers for exit-distance scaling ───────────
pub const PHI_0382: f64 = 0.382;  // 1 − φ⁻¹  → hurst trail
pub const PHI_0618: f64 = 0.618;  // φ⁻¹      → wall trail cushion
pub const PHI_2618: f64 = 2.618;  // φ²       → wall trail activate, hurst min gain
pub const PHI_6854: f64 = 6.854;  // φ⁴       → bracket TP

// ── Soft trail constants (linked to wall trail, not independently tunable) ──
pub const SOFT_TRAIL_ACTIVATE_FRAC: f64 = 0.93;  // soft trail = 93% of wall trail
pub const SOFT_TRAIL_HURST_MULT: f64 = 0.9;      // stricter Hurst threshold for early TP

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StrategyConfig {
    // ── Entry filters ────────────────────────────────────────────────────
    /// IV spike multiplier: spike detected when put_wall_iv > baseline_ema × iv_spike_mult.
    /// Default 2.0 = IV must be 2× above baseline to qualify as a spike.
    pub iv_spike_mult: f64,
    // vf_min_crash_atr + crash_lookback_days: removed (no effect in sweep)
    /// Min ATR as % of price at spike detection time. Rejects low-vol spikes early.
    /// 0.0 = disabled.
    pub spike_min_atr_pct: f64,
    /// Max bars (15-min) from spike to entry. 0 = use global constant (50).
    pub iv_lookback_bars: i64,
    /// Max TSI at spike time. 0 = disabled.
    pub spike_max_tsi: f64,
    /// Min TSI at spike time. 0 = disabled.
    pub spike_min_tsi: f64,
    /// Min wall spread (CW−PW)/ATR at spike time. 0 = disabled.
    pub spike_min_wall_spread_atr: f64,
    /// Max wall spread (CW−PW)/ATR at spike time. 0 = disabled.
    pub spike_max_wall_spread_atr: f64,
    /// Max (spot−CW)/ATR at spike time (negative = below CW). 0 = disabled.
    pub spike_max_cw_dist_atr: f64,
    /// TSI threshold for eligibility: below this, all bars in window are eligible.
    pub elig_tsi_oversold: f64,
    /// Max bars from spike when TSI is not oversold. 0 = disabled (no TSI gate).
    pub elig_early_bars: i64,

    // ── Risk management ──────────────────────────────────────────────────
    pub daily_loss_limit_pct: f64,
    pub max_entries_per_day: u32,
    /// Max concurrent open positions across all tickers. When multiple tickers
    /// signal on the same bar, entries are ranked by momentum score and only
    /// the top candidates (up to this limit) are scheduled.
    /// Position size is auto-scaled: each entry gets max_position_pct / max_open_positions.
    pub max_open_positions: u32,
    // ── Position sizing (shared by live and backtest) ──────────────────
    pub max_position_pct: f64,
    /// Exit width in ATR. Fibonacci-derived exit distances (see FIB_* constants):
    ///   SL = w, cushion = φ⁻¹·w, hurst_trail = (1−φ⁻¹)·w,
    ///   trail_activate = φ²·w, hurst_min_gain = φ²·w.
    pub exit_width_atr: f64,
    /// TSI-adaptive SL: adds/subtracts ATR from SL based on entry TSI.
    /// At TSI=-50, SL widens by this amount; at TSI=+50, SL tightens by this amount.
    /// 0.0 = disabled.
    pub sl_tsi_adapt: f64,
    // ── Hurst trend-exhaustion trail ──────────────────────────────────
    // wall_smooth_halflife: now const WALL_SMOOTH_HALFLIFE
    // atr_regime_ema_len: now const ATR_REGIME_EMA_LEN
    /// Half-life for the pure-EMA spread-gate smoother (0 = use WALL_SMOOTH_HALFLIFE).
    pub spread_smooth_halflife: usize,
    /// Hurst below this = trend exhausted → tighten stop. 0 = disabled. Default 0.45.
    pub hurst_exhaust_threshold: f64,

    /// Block new entries at or after this time of day (Eastern Time, hhmm format).
    /// Default: 1515 (3:15 PM ET). 0 = disabled.
    pub no_entry_before_et: u32,
    pub no_entry_after_et: u32,

    // ── Spike-path params ─────────────────────────────────────────────
    /// Max (narrow_cw - narrow_pw) / ATR for spike-path entry. 999 = disabled.
    pub vf_max_wall_spread_atr: f64,
    /// Dead-zone width as fraction of TSI half-range (default 1/3).
    /// Derives: lo = -w, hi = w, min_ncw_atr = 3w/100, max_adx = 200/3 - w.
    /// 0 = disabled.
    pub vf_dead_zone_width: f64,
    /// Minimum consecutive bars narrow CW must stay below smoothed CW (γ×OI-weighted)
    /// before the vf_cw_scw gate rejects. Brief dips are normal during IV compression.
    pub vf_cw_scw_persist_bars: u32,
    // hurst_min_persist_bars: now const HURST_MIN_PERSIST_BARS
    /// Min (narrow_pw - smoothed_pw) / ATR at entry. -999 = disabled.
    pub vf_min_pw_spw_atr: f64,
    /// Min ATR as % of price for VF entry. Higher vol = stronger vanna unwind.
    /// 0.0 = disabled.
    pub vf_min_atr_pct: f64,
    /// Max ATR as % of price for VF entry. Blocks entries in extreme short-term vol.
    /// 0.0 = disabled.
    pub vf_max_atr_pct: f64,
    /// Max slow ATR (250-bar EMA) as % of price. Blocks fundamentally volatile
    /// stocks in a temporary calm spell. 0.0 = disabled.
    pub vf_max_slow_atr_pct: f64,
    /// Max normalized GEX (gex / gex_abs_ema). Range ~-1 to +1. 10.0 = disabled.
    pub vf_max_gex_norm: f64,
    /// Gamma position threshold for CW gate bypass.
    /// When gamma_pos > this, CW strength check is bypassed (price is well inside the walls).
    /// 0.7 = default, 1.0 = disable bypass.
    pub vf_cw_gamma_bypass: f64,
    /// CW weakness rescue: bypass CW gate when TSI < this AND bars_since_spike <= vf_cw_rescue_bars.
    /// Combination recognizes that early+oversold entries have temporarily weak CW that recovers.
    /// 0 = disabled.
    pub vf_cw_rescue_tsi: f64,
    /// Max bars from spike for CW rescue bypass. Only active when vf_cw_rescue_tsi != 0.
    pub vf_cw_rescue_bars: i64,

    /// TSI must be below this for the IV compression gate to reject. 999 = always reject.
    pub vf_compress_tsi_max: f64,
    /// IV compress rescue: bypass when TSI <= this AND bars_since_spike <= rescue_bars.
    /// Deeply oversold + early → vanna unwind imminent even before IV compresses.
    /// 0 = disabled.
    pub vf_compress_rescue_tsi: f64,
    /// Max bars from spike for IV compress rescue. Only active when rescue_tsi != 0.
    pub vf_compress_rescue_bars: i64,
    /// Rally cap: reject when cum_return_atr > this AND bars_since_spike > rally_min_bars.
    /// Catches late entries where vanna unwind is already spent. 0 = disabled.
    pub vf_max_rally_atr: f64,
    /// Rally cap: entries before this many bars from spike are exempt.
    pub vf_rally_min_bars: i64,
    /// TSI time decay: each bar from spike lowers max TSI by this amount.
    /// Early entries get full TSI headroom; late entries must be more oversold. 0 = disabled.
    pub vf_tsi_time_decay: f64,

    /// Min net vanna at spike time for VF entry (positive = bullish flow on IV drop).
    /// 0.0 = disabled.
    pub vf_min_spike_vanna: f64,
    /// Min gamma tilt at spike time (positive = call-dominant, dealers dampen).
    /// 0.0 = disabled. Range [-1, 1].
    pub vf_min_spike_gamma_tilt: f64,
    /// Min PW drift since spike in ATR units (positive = PW rising = bullish).
    /// 0.0 = disabled.
    pub vf_min_pw_drift_atr: f64,
    /// Min gamma tilt at entry time. 0.0 = disabled.
    pub vf_min_gamma_tilt: f64,


    // ── WallBounce (calm-path) ───────────────────────────────────────────
    /// Min zone dwell score for WB entry (narrow walls).
    pub wb_min_zone_score: f64,
    
    /// Min wall spread in ATR for WB.
    pub wb_min_wall_spread_atr: f64,
    /// Min TSI for WB entry. Ensures momentum is modestly positive, not just a fleeting cross.
    pub wb_min_tsi: f64,
    /// Min EMA fast−slow gap as % of slow EMA. Requires meaningful trend, not a touch-and-go cross.
    /// 0.0 = just requires fast > slow (legacy). Typical: 0.05-0.20%.
    pub wb_min_ema_gap_pct: f64,
    
    /// WB take-profit = CW distance × this multiplier. 0 = use bracket_tp_atr().
    pub wb_tp_spread_mult: f64,
    

    /// Wall trail cushion below put wall (ATR).
    pub wall_trail_cushion_atr: f64,
    // wall_trail_activate_atr: always PHI_2618 × exit_width_atr
    // hurst_min_gain_atr: always PHI_2618 × exit_width_atr

    /// TP proximity trail: fraction of TP distance at which tight trail activates (0 = disabled).
    pub tp_proximity_trigger: f64,

    /// Profit floor: once highest-close runup exceeds this (ATR), start trailing. 0 = disabled.
    pub profit_floor_activate_atr: f64,
    /// Profit floor trail width from highest close (ATR). Should be >= SL width.
    pub profit_floor_trail_atr: f64,


    // ── Soft trail (early breakeven protection) ─────────────────────────────
    // ── Early Hurst TP (exit on reversal before wall trail activates) ────────
    // soft_trail_activate_atr: always 0.93 × wall_trail_activate_atr (linked, not tunable)
    // soft_trail_hurst_mult: always SOFT_TRAIL_HURST_MULT constant (linked, not tunable)
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            iv_spike_mult: 3.5,
            spike_min_atr_pct: 0.30,
            iv_lookback_bars: 0,
            spike_max_tsi: 0.0,
            spike_min_tsi: 0.0,
            spike_min_wall_spread_atr: 0.0,
            spike_max_wall_spread_atr: 0.0,
            spike_max_cw_dist_atr: 0.0,
            elig_tsi_oversold: -5.0,
            elig_early_bars: 0,
            daily_loss_limit_pct: 0.15,
            max_entries_per_day: 10,
            max_open_positions: Self::default_max_open_positions(),
            max_position_pct: 0.95,
            exit_width_atr: 4.5,
            sl_tsi_adapt: 1.625,
            spread_smooth_halflife: 25,
            hurst_exhaust_threshold: 0.45,
            no_entry_before_et: 1030,
            no_entry_after_et: 1500,
            vf_max_wall_spread_atr: 10.0,
            vf_dead_zone_width: TSI_RANGE / 3.0,
            vf_cw_scw_persist_bars: 5,
            vf_min_pw_spw_atr: -1.5,
            vf_min_atr_pct: 0.30,
            vf_max_atr_pct: 0.50,
            vf_max_slow_atr_pct: 0.55,
            vf_max_gex_norm: 2.0,
            vf_cw_gamma_bypass: 0.70,
            vf_cw_rescue_tsi: 0.0,
            vf_cw_rescue_bars: 0,
            vf_compress_tsi_max: 0.0,
            vf_compress_rescue_tsi: 0.0,
            vf_compress_rescue_bars: 0,
            vf_max_rally_atr: 0.0,
            vf_rally_min_bars: 0,
            vf_tsi_time_decay: 0.0,
            vf_min_spike_vanna: 0.0,
            vf_min_spike_gamma_tilt: 0.0,
            vf_min_pw_drift_atr: 0.0,
            vf_min_gamma_tilt: 0.0,
            wb_min_zone_score: 1.5,
            wb_min_wall_spread_atr: 5.0,
            wb_min_tsi: 20.0,
            wb_min_ema_gap_pct: 0.06,
            wb_tp_spread_mult: 0.95,
            wall_trail_cushion_atr: 3.0,
            tp_proximity_trigger: 0.85,
            profit_floor_activate_atr: 0.0,
            profit_floor_trail_atr: 0.0,
        }
    }
}

impl StrategyConfig {
    pub const fn default_max_open_positions() -> u32 { 3 }

    /// Check if an ET hhmm value (e.g. 1030 for 10:30 AM) falls within the
    /// allowed entry window. Shared by `CanEnterGate` and missed-entries filter.
    pub fn in_entry_time_window(&self, hhmm: u32) -> bool {
        if self.no_entry_before_et > 0 && hhmm < self.no_entry_before_et { return false; }
        if self.no_entry_after_et > 0 && hhmm >= self.no_entry_after_et { return false; }
        true
    }

    pub fn dead_zone_lo(&self) -> f64 { -self.vf_dead_zone_width }
    pub fn dead_zone_hi(&self) -> f64 { self.vf_dead_zone_width }
    pub fn dead_zone_min_ncw_atr(&self) -> f64 { 3.0 * self.vf_dead_zone_width / TSI_RANGE }
    pub fn dead_zone_max_adx(&self) -> f64 { 2.0 * TSI_RANGE / 3.0 - self.vf_dead_zone_width }

    pub fn bracket_sl_atr(&self) -> f64 { self.exit_width_atr }
    pub fn bracket_tp_atr(&self) -> f64 { PHI_6854 * self.exit_width_atr }
    pub fn wall_trail_cushion_atr(&self) -> f64 { self.wall_trail_cushion_atr }
    pub fn hurst_trail_atr(&self) -> f64 { PHI_0382 * self.exit_width_atr }
    pub fn wall_trail_activate_atr(&self) -> f64 { PHI_2618 * self.exit_width_atr }
    pub fn soft_trail_activate_atr(&self) -> f64 { SOFT_TRAIL_ACTIVATE_FRAC * self.wall_trail_activate_atr() }
    pub fn hurst_min_gain_atr(&self) -> f64 { PHI_2618 * self.exit_width_atr }
    pub fn wb_max_wall_spread_atr(&self) -> f64 { 4.0 * self.exit_width_atr }

    pub fn tp_proximity_trigger(&self) -> f64 { self.tp_proximity_trigger }

    /// Effective IV lookback window (bars). Config value if set, else global constant.
    pub fn eff_iv_lookback_bars(&self) -> BarIndex {
        if self.iv_lookback_bars > 0 { self.iv_lookback_bars } else { IV_LOOKBACK_BARS_INDEX }
    }

    /// Effective IV spike multiplier, regime-adaptive: base × min(1.05, max(1, regime)^0.25).
    pub fn eff_iv_spike_mult(&self, atr_regime_ratio: f64) -> f64 {
        let scale = atr_regime_ratio.max(1.0).powf(0.65).min(1.05);
        self.iv_spike_mult * scale
    }

    /// Effective hurst exhaustion threshold, regime-adaptive: base × max(1, regime)^0.5.
    pub fn eff_hurst_exhaust_threshold(&self, atr_regime_ratio: f64) -> f64 {
        self.hurst_exhaust_threshold * atr_regime_ratio.max(1.0).sqrt()
    }

    /// Effective PW-SPW threshold, regime-adaptive: base / max(1, regime)^0.5.
    /// High vol → tighter (less negative) threshold rejects noisy PW dislocations.
    pub fn eff_pw_spw_threshold(&self, atr_regime_ratio: f64) -> f64 {
        self.vf_min_pw_spw_atr / atr_regime_ratio.max(1.0).sqrt()
    }

    /// Effective max TSI for VF entry, regime-adaptive: (TSI_RANGE × HALF) / max(1, regime)^HALF².
    /// High vol → lower cap blocks overbought chasing; calm vol → full base applies.
    pub fn eff_vf_max_tsi(atr_regime_ratio: f64) -> f64 {
        TSI_RANGE * HALF / atr_regime_ratio.max(1.0).powf(HALF * HALF)
    }

    /// Effective GEX norm threshold, regime-adaptive: base × max(1, regime)^0.5.
    /// High vol → higher GEX allowed (vol correlates with extreme GEX).
    /// This structural link prevents independent tuning of gex_norm and regime_ema.
    pub fn eff_gex_norm_threshold(&self, atr_regime_ratio: f64) -> f64 {
        self.vf_max_gex_norm * atr_regime_ratio.max(1.0).sqrt()
    }

    /// Max concurrent positions (clamped to ≥1). Same first component as [`Self::slot_params`].
    #[inline]
    pub fn max_open_slots(&self) -> usize {
        self.max_open_positions.max(1) as usize
    }

    /// Max concurrent slots and position-sizing divisor.
    pub fn slot_params(&self) -> (usize, f64) {
        let max_pos = self.max_open_slots();
        (max_pos, max_pos as f64)
    }

    /// Minimum number of strategy bars needed before indicators are reliable.
    pub fn min_indicator_bars(&self) -> usize {
        *[
            EMA_SLOW_LEN + 1,
            IV_LOOKBACK_BARS + 1,
            DI_LENGTH * 3,
            TSI_LONG_LENGTH + TSI_SHORT_LENGTH + TSI_SIGNAL_LENGTH + 2,
        ]
        .iter()
        .max()
        .expect("min_indicator_bars: non-empty array")
    }

    /// Minimum GEX-matched signal bars needed during warmup (bar-granularity).
    /// Day-granularity IV baseline EMA is guaranteed by `warmup_trading_days`.
    pub fn min_signal_bars(&self) -> usize {
        self.spread_smooth_halflife
    }

    fn trading_to_calendar(td: usize) -> u32 {
        (td as f64 * 7.0 / 5.0).ceil() as u32 + 5
    }

    fn gex_trading_days(&self) -> usize {
        let bars_per_day = super::bar_interval::bars_per_day_1m_div_interval(
            crate::types::BARS_PER_DAY,
            BAR_INTERVAL_MINUTES,
        );
        let indicator_days = self.min_indicator_bars() / bars_per_day + 2;
        indicator_days.max(IV_BASELINE_EMA_DAYS)
    }

    /// Trading days for GEX phase (IBKR treats Duration::days as trading days).
    pub fn warmup_gex_trading_days(&self) -> u32 {
        self.gex_trading_days() as u32
    }

    /// Calendar days for warmup (GEX + bars).
    pub fn warmup_gex_calendar_days(&self) -> u32 {
        Self::trading_to_calendar(self.gex_trading_days())
    }

    /// Split trading days into warmup phases given the first trading day after
    /// warmup (`trading_start`). Exclusive — `trading_start` itself is NOT
    /// included in warmup.
    ///
    /// Used by backtest runner, live recovery, and parity tests.
    pub fn warmup_day_split(&self, trading_start: chrono::NaiveDate) -> WarmupSplit {
        use crate::backtest::calendar::get_trading_days;

        let cal = i64::from(self.warmup_gex_calendar_days());
        let gex_begin = trading_start - chrono::Duration::days(cal);

        let begin_str = gex_begin.format(crate::types::DATE_FMT).to_string();
        let end_str = (trading_start - chrono::Duration::days(1))
            .format(crate::types::DATE_FMT).to_string();

        WarmupSplit {
            gex_days: get_trading_days(&begin_str, &end_str),
            gex_boundary: gex_begin,
        }
    }

}

pub struct WarmupSplit {
    pub gex_days: Vec<String>,
    /// First calendar date of the warmup. Used by live as IBKR phase-1 end.
    pub gex_boundary: chrono::NaiveDate,
}


// ── CLI overrides ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, clap::Args)]
pub struct StrategyOverrides {
    /// Exit width in ATR (derives SL, cushion, trail distances)
    #[arg(long)]
    pub exit_width_atr: Option<f64>,

    /// TSI-adaptive SL scaling factor (0=disabled)
    #[arg(long)]
    pub sl_tsi_adapt: Option<f64>,





    /// Pure-EMA spread-gate smoother half-life in bars (0=use wall_smooth_halflife)
    #[arg(long)]
    pub spread_smooth_halflife: Option<usize>,

    /// Hurst exhaustion threshold (0=disabled, e.g. 0.45)
    #[arg(long)]
    pub hurst_exhaust_threshold: Option<f64>,

    /// Max entries per day
    #[arg(long)]
    pub max_entries_per_day: Option<u32>,

    /// Daily loss limit as fraction (e.g. 0.15)
    #[arg(long)]
    pub daily_loss_limit_pct: Option<f64>,

    /// Max position size as fraction of equity
    #[arg(long)]
    pub max_position_pct: Option<f64>,

    /// Block entries before this ET time (hhmm, 0=disabled)
    #[arg(long)]
    pub no_entry_before_et: Option<u32>,


    // ── Spike-path signal ────────────────────────────────────────────

    /// Max narrow-wall spread in ATR for VF entry (999=disabled)
    #[arg(long)]
    pub vf_max_wall_spread_atr: Option<f64>,

    /// Dead zone width in TSI units (derives lo/hi/ncw/adx, 0=disabled)
    #[arg(long)]
    pub vf_dead_zone_width: Option<f64>,

    /// Bars CW must persist below smooth before rejecting (default 6)
    #[arg(long)]
    pub vf_cw_scw_persist_bars: Option<u32>,

    /// Min (narrow_pw - smoothed_pw)/ATR at entry (-999=disabled)
    #[arg(long)]
    pub vf_min_pw_spw_atr: Option<f64>,

    /// Min ATR as % of price for VF entry (0=disabled)
    #[arg(long)]
    pub vf_min_atr_pct: Option<f64>,
    /// Max ATR as % of price for VF entry (0=disabled)
    #[arg(long)]
    pub vf_max_atr_pct: Option<f64>,
    /// Max slow ATR (250-bar EMA) as % of price (0=disabled)
    #[arg(long)]
    pub vf_max_slow_atr_pct: Option<f64>,
    /// Max normalized GEX (10.0=disabled)
    #[arg(long)]
    pub vf_max_gex_norm: Option<f64>,
    /// Gamma position threshold for CW gate bypass (0.7=default, 1.0=disable bypass)
    #[arg(long)]
    pub vf_cw_gamma_bypass: Option<f64>,
    /// CW rescue: bypass when TSI < this AND bars <= rescue_bars (0=disabled)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_cw_rescue_tsi: Option<f64>,
    /// CW rescue: max bars from spike (0=disabled)
    #[arg(long)]
    pub vf_cw_rescue_bars: Option<i64>,
    /// Block entries at or after this ET time (hhmm, 0=disabled)
    #[arg(long)]
    pub no_entry_after_et: Option<u32>,

    /// Min ATR % at spike detection time (0=disabled)
    #[arg(long)]
    pub spike_min_atr_pct: Option<f64>,

    /// Max bars from spike to entry (0=use default 50)
    #[arg(long)]
    pub iv_lookback_bars: Option<i64>,

    /// Max TSI at spike time (0=disabled)
    #[arg(long)]
    pub spike_max_tsi: Option<f64>,

    /// Min TSI at spike time (0=disabled)
    #[arg(long)]
    pub spike_min_tsi: Option<f64>,

    /// Min wall spread at spike (0=disabled)
    #[arg(long)]
    pub spike_min_wall_spread_atr: Option<f64>,

    /// Max wall spread at spike (0=disabled)
    #[arg(long)]
    pub spike_max_wall_spread_atr: Option<f64>,

    /// Max (spot-CW)/ATR at spike (0=disabled)
    #[arg(long)]
    pub spike_max_cw_dist_atr: Option<f64>,

    /// TSI threshold for eligibility gate (default -5)
    #[arg(long)]
    pub elig_tsi_oversold: Option<f64>,

    /// Max bars from spike when TSI not oversold (0=disable gate)
    #[arg(long)]
    pub elig_early_bars: Option<i64>,

    // ── IV parameters ─────────────────────────────────────────────────
    /// IV spike mult: spike when put_wall_iv > baseline × mult (default 2.0)
    #[arg(long)]
    pub iv_spike_mult: Option<f64>,

    /// TSI ceiling for IV compression gate (default 5)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_compress_tsi_max: Option<f64>,
    /// IV compress rescue: bypass when TSI <= this (0=disabled)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_compress_rescue_tsi: Option<f64>,
    /// IV compress rescue: max bars from spike (0=disabled)
    #[arg(long)]
    pub vf_compress_rescue_bars: Option<i64>,
    /// Rally cap: max cum_return_atr before rejecting late entries (0=disabled)
    #[arg(long)]
    pub vf_max_rally_atr: Option<f64>,
    /// Rally cap: exempt entries within this many bars from spike
    #[arg(long)]
    pub vf_rally_min_bars: Option<i64>,
    /// TSI time decay per bar from spike (0=disabled)
    #[arg(long)]
    pub vf_tsi_time_decay: Option<f64>,

    /// Min net vanna at spike time (0=disabled)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_min_spike_vanna: Option<f64>,
    /// Min gamma tilt at spike time (0=disabled, range -1..1)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_min_spike_gamma_tilt: Option<f64>,
    /// Min PW drift since spike in ATR (0=disabled)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_min_pw_drift_atr: Option<f64>,
    /// Min gamma tilt at entry time (0=disabled)
    #[arg(long, allow_hyphen_values = true)]
    pub vf_min_gamma_tilt: Option<f64>,

    // ── WallBounce (calm-path) ──────────────────────────────────────

    /// Min zone score for WB entry (default 1.5)
    #[arg(long)]
    pub wb_min_zone_score: Option<f64>,

    /// Min wall spread in ATR for WB (default 5.0)
    #[arg(long)]
    pub wb_min_wall_spread_atr: Option<f64>,
    /// Min TSI for WB entry (default 20)
    #[arg(long, allow_hyphen_values = true)]
    pub wb_min_tsi: Option<f64>,
    /// Min EMA gap % for WB (default 0)
    #[arg(long)]
    pub wb_min_ema_gap_pct: Option<f64>,

    /// WB TP = CW dist × mult (default 2.0, 0 = global TP)
    #[arg(long)]
    pub wb_tp_spread_mult: Option<f64>,

    /// Wall trail cushion below put wall (ATR).
    #[arg(long)]
    pub wall_trail_cushion_atr: Option<f64>,

    /// TP proximity trail trigger (fraction of TP distance, 0 = disabled).
    #[arg(long)]
    pub tp_proximity_trigger: Option<f64>,

    /// Profit floor: activate trailing after this runup (ATR). 0 = disabled.
    #[arg(long)]
    pub profit_floor_activate_atr: Option<f64>,

    /// Profit floor trail width from highest close (ATR).
    #[arg(long)]
    pub profit_floor_trail_atr: Option<f64>,


}

impl StrategyOverrides {
    pub fn apply(self, cfg: &mut StrategyConfig) {
        macro_rules! set {
            ($($field:ident),+ $(,)?) => {
                $(if let Some(v) = self.$field {
                    cfg.$field = v;
                })+
            };
        }

        set!(
            exit_width_atr, sl_tsi_adapt, wall_trail_cushion_atr,
            tp_proximity_trigger, profit_floor_activate_atr, profit_floor_trail_atr,
            spread_smooth_halflife,
            hurst_exhaust_threshold, vf_cw_scw_persist_bars,
            max_entries_per_day, daily_loss_limit_pct, max_position_pct,
            no_entry_before_et, no_entry_after_et,
            vf_min_atr_pct, vf_max_atr_pct, vf_max_slow_atr_pct, vf_max_gex_norm,
            vf_cw_gamma_bypass, vf_cw_rescue_tsi, vf_cw_rescue_bars, vf_dead_zone_width,
            vf_max_wall_spread_atr, vf_min_pw_spw_atr,
            spike_min_atr_pct, iv_lookback_bars, spike_max_tsi, spike_min_tsi,
            spike_min_wall_spread_atr, spike_max_wall_spread_atr, spike_max_cw_dist_atr,
            elig_tsi_oversold, elig_early_bars,
            iv_spike_mult, vf_compress_tsi_max, vf_compress_rescue_tsi, vf_compress_rescue_bars,
            vf_max_rally_atr, vf_rally_min_bars, vf_tsi_time_decay,
            vf_min_spike_vanna, vf_min_spike_gamma_tilt,
            vf_min_pw_drift_atr, vf_min_gamma_tilt,
            wb_min_zone_score, wb_min_wall_spread_atr, wb_min_tsi, wb_min_ema_gap_pct, wb_tp_spread_mult,
        );

        cfg.validate_and_clamp();
    }
}

impl StrategyConfig {
    /// Clamp tunable params to safe plateau ranges. Called after CLI overrides.
    /// Values outside these ranges have been shown to cause H1/H2 divergence.
    pub fn validate_and_clamp(&mut self) {
        self.validate_and_clamp_quiet(false);
    }

    pub fn validate_and_clamp_quiet(&mut self, quiet: bool) {
        if self.hurst_exhaust_threshold > 0.0 && self.hurst_exhaust_threshold < 0.40 {
            if !quiet { eprintln!("[warn] hurst_exhaust_threshold={:.2} clamped to 0.40 (below plateau)", self.hurst_exhaust_threshold); }
            self.hurst_exhaust_threshold = 0.40;
        }
        if self.vf_max_gex_norm < 1.5 {
            if !quiet { eprintln!("[warn] vf_max_gex_norm={:.2} clamped to 1.5 (below plateau)", self.vf_max_gex_norm); }
            self.vf_max_gex_norm = 1.5;
        }
        if self.spike_min_atr_pct > 0.35 {
            if !quiet { eprintln!("[warn] spike_min_atr_pct={:.2} clamped to 0.35 (above plateau)", self.spike_min_atr_pct); }
            self.spike_min_atr_pct = 0.35;
        }
        if self.vf_max_slow_atr_pct > 0.0 && self.vf_max_slow_atr_pct < 0.40 {
            if !quiet { eprintln!("[warn] vf_max_slow_atr_pct={:.2} clamped to 0.40 (below plateau)", self.vf_max_slow_atr_pct); }
            self.vf_max_slow_atr_pct = 0.40;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub starting_capital: f64,
    pub commission_per_share: f64,
    pub commission_min: f64,
    pub slippage_ticks: f64,
    pub execution_delay_bars: i32,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            starting_capital: 10_000.0,
            commission_per_share: 0.005,
            commission_min: 1.00,
            slippage_ticks: 10.0,
            execution_delay_bars: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_constants_sane() {
        assert!(ATR_LENGTH > 0);
        assert!(EMA_FAST_LEN > 0);
        assert!(EMA_FAST_LEN < EMA_SLOW_LEN);
        assert!(DI_LENGTH > 0);
        assert!(IV_LOOKBACK_BARS > 0);
    }

    #[test]
    fn backtest_defaults_sane() {
        let c = BacktestConfig::default();
        assert!(c.starting_capital > 0.0);
        assert!(c.commission_per_share > 0.0);
        assert!(c.slippage_ticks >= 0.0);
        assert!(c.execution_delay_bars >= 0);
    }

    #[test]
    fn strategy_config_serde_roundtrip() {
        let c = StrategyConfig::default();
        let json = serde_json::to_string(&c).unwrap();
        let c2: StrategyConfig = serde_json::from_str(&json).unwrap();
        assert!((c.exit_width_atr - c2.exit_width_atr).abs() < 0.01);
    }

    #[test]
    fn slot_params_default() {
        let c = StrategyConfig::default();
        let (max_pos, div) = c.slot_params();
        assert_eq!(max_pos, c.max_open_slots());
        assert_eq!(max_pos, c.max_open_positions.max(1) as usize);
        assert!((div - max_pos as f64).abs() < 0.01);
    }

    #[test]
    fn slot_params_zero_clamps_to_one() {
        let mut c = StrategyConfig::default();
        c.max_open_positions = 0;
        let (max_pos, div) = c.slot_params();
        assert_eq!(c.max_open_slots(), 1);
        assert_eq!(max_pos, 1);
        assert!((div - 1.0).abs() < 0.01);
    }

    #[test]
    fn warmup_day_split_phases_are_contiguous_and_non_overlapping() {
        let c = StrategyConfig::default();
        let trading_start = chrono::NaiveDate::from_ymd_opt(2024, 6, 3).unwrap();
        let split = c.warmup_day_split(trading_start);

        assert!(!split.gex_days.is_empty(), "warmup must have days");

        let first_p2 = split.gex_days.first().unwrap();
        let boundary_str = split.gex_boundary.format(crate::types::DATE_FMT).to_string();
        assert!(boundary_str <= *first_p2, "gex_boundary must be <= first gex trading day");

        // No day is at or after trading_start
        let start_str = trading_start.format(crate::types::DATE_FMT).to_string();
        assert!(split.gex_days.last().unwrap() < &start_str, "warmup must exclude trading_start");
    }

    #[test]
    fn warmup_day_split_deterministic_across_dates() {
        let c = StrategyConfig::default();
        let dates = [
            chrono::NaiveDate::from_ymd_opt(2024, 6, 3).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2025, 3, 10).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2023, 1, 9).unwrap(),
        ];
        for d in dates {
            let a = c.warmup_day_split(d);
            let b = c.warmup_day_split(d);
            
            assert_eq!(a.gex_days, b.gex_days, "p2 mismatch for {d}");
            assert_eq!(a.gex_boundary, b.gex_boundary, "boundary mismatch for {d}");
        }
    }
}
