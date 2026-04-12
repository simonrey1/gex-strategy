use crate::config::strategy::HALF;
use crate::config::BarIndex;
use crate::config::StrategyConfig;
use crate::strategy::indicators::IndicatorValues;
use crate::types::VfGate;
use serde::Serialize;
use ts_rs::TS;

use super::BarCtx;

/// TSI / ADX / regime-ratio triple shared across indicator, gate, and scan contexts.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "shared/generated/")]
pub struct RegimeCtx {
    pub tsi: f64,
    pub adx: f64,
    pub atr_regime_ratio: f64,
}

impl From<&IndicatorValues> for RegimeCtx {
    fn from(ind: &IndicatorValues) -> Self {
        Self { tsi: ind.tsi, adx: ind.adx, atr_regime_ratio: ind.atr_regime_ratio }
    }
}

/// IV compression ratio + config — for [`VfGateCtx::passes_iv_compress`] / [`VfGateCtx::passes_all`].
#[derive(Debug, Clone, Copy)]
pub struct VfCompressParams<'a> {
    pub compress_ratio: f64,
    pub config: &'a StrategyConfig,
}

impl<'a> VfCompressParams<'a> {
    #[inline]
    pub const fn new(compress_ratio: f64, config: &'a StrategyConfig) -> Self {
        Self { compress_ratio, config }
    }
}

/// Shared gate-check inputs for VannaFlip entry.
/// Embedded in `ScanSnapshot` (flattened) and built on the fly by `VfCtx`.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "shared/generated/")]
pub struct VfGateCtx {
    #[serde(flatten)]
    #[ts(flatten)]
    pub regime: RegimeCtx,
    pub atr_pct: f64,
    /// EMA(ATR, 250) as % of spot — slow vol baseline.
    pub slow_atr_pct: f64,
    pub cw_vs_scw_atr: Option<f64>,
    pub pw_vs_spw_atr: Option<f64>,
    pub net_gex: f64,
    pub gex_abs_ema: f64,
    pub bars_since_spike: BarIndex,
    pub wall_spread_atr: Option<f64>,
    /// (close − smoothed_pw) / (smoothed_cw − smoothed_pw), clamped 0..1.
    pub gamma_pos: f64,
    /// Net vanna at spike time (positive = IV drop → dealer buy shares).
    pub spike_vanna: f64,
    /// Gamma tilt at spike time: (call_goi − put_goi) / total. Positive = dealers dampen.
    pub spike_gamma_tilt: f64,
    /// Put wall drift since spike in ATR units. Positive = PW rising (bullish).
    pub pw_drift_atr: f64,
    /// Current net vanna (entry time).
    pub net_vanna: f64,
    /// Current gamma tilt (entry time).
    pub gamma_tilt: f64,
    /// Cumulative price return since spike in ATR units. Negative = still down, positive = already rallied.
    pub cum_return_atr: f64,
}

/// Compute gamma position: (close − pw) / (cw − pw), clamped to [0, 1].
/// 0.5 fallback when walls are invalid.
#[inline]
pub fn compute_gamma_pos(close: f64, pw: f64, cw: f64) -> f64 {
    if cw > pw && pw > 0.0 {
        ((close - pw) / (cw - pw)).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

impl<'a> From<&'a BarCtx<'a>> for VfGateCtx {
    fn from(ctx: &'a BarCtx<'a>) -> Self {
        let s = ctx.state;
        let wd = ctx.wall_diff_atr_ref();
        let (atr_pct, slow_atr_pct) = ctx.vf_atr_pct_pair();
        let spw = s.smoothed_put_wall();
        let scw = s.smoothed_call_wall();
        Self {
            regime: ctx.regime_ctx(),
            atr_pct,
            slow_atr_pct,
            cw_vs_scw_atr: Some(wd.cw_scw_atr),
            pw_vs_spw_atr: Some(wd.pw_spw_atr),
            net_gex: ctx.net_gex(),
            gex_abs_ema: s.gex_abs_ema,
            bars_since_spike: s.bars_since_spike(),
            wall_spread_atr: Some(s.spread_atr()),
            gamma_pos: compute_gamma_pos(ctx.bar.close, spw, scw),
            spike_vanna: s.spike_net_vanna,
            spike_gamma_tilt: s.spike_gamma_tilt,
            pw_drift_atr: s.pw_drift_atr(ctx.ind.atr),
            net_vanna: ctx.gex.net_vanna,
            gamma_tilt: ctx.gex.gamma_tilt,
            cum_return_atr: s.spike_cum_return_atr,
        }
    }
}

impl VfGateCtx {
    pub fn passes_atr_min(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_min_atr_pct <= 0.0 || self.atr_pct >= cfg.vf_min_atr_pct
    }

    pub fn passes_atr_max(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_max_atr_pct <= 0.0 || self.atr_pct < cfg.vf_max_atr_pct
    }

    pub fn passes_slow_atr_max(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_max_slow_atr_pct <= 0.0 || self.slow_atr_pct < cfg.vf_max_slow_atr_pct
    }

    pub fn passes_tsi_max(&self, cfg: &StrategyConfig) -> bool {
        let base = StrategyConfig::eff_vf_max_tsi(self.regime.atr_regime_ratio);
        let eff = if cfg.vf_tsi_time_decay > 0.0 {
            base - self.bars_since_spike as f64 * cfg.vf_tsi_time_decay
        } else {
            base
        };
        self.regime.tsi <= eff
    }

    /// ADX sub-check of the dead-zone gate. VfCtx additionally checks ncw_gap.
    pub fn passes_dead_zone_adx(&self, cfg: &StrategyConfig) -> bool {
        let dz_lo = cfg.dead_zone_lo();
        let dz_hi = cfg.dead_zone_hi();
        if dz_lo < dz_hi && self.regime.tsi >= dz_lo && self.regime.tsi < dz_hi {
            let max_adx = cfg.dead_zone_max_adx() / self.regime.atr_regime_ratio.max(1.0).powf(HALF);
            max_adx <= 0.0 || self.regime.adx < max_adx
        } else {
            true
        }
    }

    pub fn passes_cw_strength(&self, cfg: &StrategyConfig) -> bool {
        if self.gamma_pos > cfg.vf_cw_gamma_bypass { return true; }
        if cfg.vf_cw_rescue_tsi != 0.0
            && self.regime.tsi < cfg.vf_cw_rescue_tsi
            && self.bars_since_spike <= cfg.vf_cw_rescue_bars as BarIndex
        {
            return true;
        }
        self.cw_vs_scw_atr.map(|d| d >= HALF).unwrap_or(true)
    }

    pub fn passes_pw_strength(&self, cfg: &StrategyConfig) -> bool {
        self.pw_vs_spw_atr
            .map(|gap| gap >= cfg.eff_pw_spw_threshold(self.regime.atr_regime_ratio))
            .unwrap_or(true)
    }

    pub fn passes_gex_norm(&self, cfg: &StrategyConfig) -> bool {
        if cfg.vf_max_gex_norm >= 10.0 || self.gex_abs_ema <= 1.0 { return true; }
        let gn = self.net_gex / self.gex_abs_ema;
        gn <= cfg.eff_gex_norm_threshold(self.regime.atr_regime_ratio)
    }

    /// `compress_ratio` is pre-computed (ScanSnapshot) or from `compress_ratio(iv_now)` in live VF.
    /// Three ways to pass: (1) IV has compressed, (2) TSI turned positive,
    /// (3) deeply oversold + early in window → vanna unwind imminent.
    pub fn passes_iv_compress(&self, p: &VfCompressParams<'_>) -> bool {
        if p.compress_ratio <= HALF { return true; }
        if self.regime.tsi >= p.config.vf_compress_tsi_max { return true; }
        p.config.vf_compress_rescue_tsi != 0.0
            && self.regime.tsi <= p.config.vf_compress_rescue_tsi
            && self.bars_since_spike <= p.config.vf_compress_rescue_bars as BarIndex
    }

    pub fn passes_spike_age(&self, cfg: &StrategyConfig) -> bool {
        self.bars_since_spike <= cfg.eff_iv_lookback_bars()
    }

    pub fn passes_wall_spread(&self, cfg: &StrategyConfig) -> bool {
        self.wall_spread_atr.map(|s| s <= cfg.vf_max_wall_spread_atr).unwrap_or(true)
    }

    /// Rejects late entries where price has already rallied from the spike.
    /// Combination: cum_return_atr > threshold AND bars_since_spike > min_bars.
    /// Catches "vanna already spent" setups.
    pub fn passes_rally_cap(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_max_rally_atr == 0.0
            || self.bars_since_spike <= cfg.vf_rally_min_bars as BarIndex
            || self.cum_return_atr <= cfg.vf_max_rally_atr
    }

    pub fn passes_spike_vanna(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_min_spike_vanna == 0.0 || self.spike_vanna >= cfg.vf_min_spike_vanna
    }

    pub fn passes_spike_gamma_tilt(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_min_spike_gamma_tilt == 0.0 || self.spike_gamma_tilt >= cfg.vf_min_spike_gamma_tilt
    }

    pub fn passes_pw_drift(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_min_pw_drift_atr == 0.0 || self.pw_drift_atr >= cfg.vf_min_pw_drift_atr
    }

    pub fn passes_gamma_tilt(&self, cfg: &StrategyConfig) -> bool {
        cfg.vf_min_gamma_tilt == 0.0 || self.gamma_tilt >= cfg.vf_min_gamma_tilt
    }

    pub fn passes_all(&self, compress: &VfCompressParams<'_>) -> bool {
        VfGate::SCAN_GATES.iter().all(|g| self.check_gate(*g, compress.config, compress))
    }

    /// Check a single gate. Exhaustive match — compiler enforces coverage of new variants.
    pub fn check_gate(&self, gate: VfGate, cfg: &StrategyConfig, compress: &VfCompressParams<'_>) -> bool {
        match gate {
            VfGate::AtrPct      => self.passes_atr_min(cfg) && self.passes_atr_max(cfg),
            VfGate::SlowAtr     => self.passes_slow_atr_max(cfg),
            VfGate::Tsi         => self.passes_tsi_max(cfg),
            VfGate::TsiDead     => self.passes_dead_zone_adx(cfg),
            VfGate::CwWeak      => self.passes_cw_strength(cfg),
            VfGate::PwWeak      => self.passes_pw_strength(cfg),
            VfGate::SpreadWide  => self.passes_wall_spread(cfg),
            VfGate::GexNorm     => self.passes_gex_norm(cfg),
            VfGate::IvCompress  => self.passes_iv_compress(compress),
            VfGate::SpikeExpired => self.passes_spike_age(cfg),
            VfGate::SpikeVanna    => self.passes_spike_vanna(cfg),
            VfGate::SpikeGammaTilt => self.passes_spike_gamma_tilt(cfg),
            VfGate::PwDrift       => self.passes_pw_drift(cfg),
            VfGate::GammaTilt     => self.passes_gamma_tilt(cfg),
            VfGate::RallyCap      => self.passes_rally_cap(cfg),
            // Preconditions / state-dependent — not checkable from VfGateCtx alone
            VfGate::NoPw | VfGate::NoSpike | VfGate::NoBaseline
            | VfGate::IvHigh | VfGate::NoIv => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_ctx() -> VfGateCtx {
        VfGateCtx {
            regime: RegimeCtx { tsi: -20.0, adx: 20.0, atr_regime_ratio: 1.0 },
            atr_pct: 0.30,
            slow_atr_pct: 0.35,
            cw_vs_scw_atr: Some(1.0),
            pw_vs_spw_atr: Some(-1.0),
            net_gex: -1e8,
            gex_abs_ema: 1e8,
            bars_since_spike: 5,
            wall_spread_atr: Some(4.0),
            gamma_pos: 0.5,
            spike_vanna: 0.0,
            spike_gamma_tilt: 0.0,
            pw_drift_atr: 0.0,
            net_vanna: 0.0,
            gamma_tilt: 0.0,
            cum_return_atr: 0.0,
        }
    }

    fn cfg() -> StrategyConfig { StrategyConfig::default() }

    fn compress(cfg: &StrategyConfig) -> VfCompressParams<'_> {
        VfCompressParams::new(0.40, cfg)
    }

    #[test]
    fn baseline_passes_all() {
        let c = cfg();
        let ctx = base_ctx();
        assert!(ctx.passes_all(&compress(&c)));
    }

    #[test]
    fn atr_pct_too_low_fails() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.atr_pct = 0.01;
        assert!(!ctx.check_gate(VfGate::AtrPct, &c, &compress(&c)));
    }

    #[test]
    fn atr_pct_too_high_fails() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.atr_pct = 10.0;
        assert!(!ctx.check_gate(VfGate::AtrPct, &c, &compress(&c)));
    }

    #[test]
    fn cw_weak_fails_when_cw_below_scw() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.gamma_pos = 0.0;
        ctx.cw_vs_scw_atr = Some(-5.0);
        assert!(!ctx.passes_cw_strength(&c));
    }

    #[test]
    fn cw_weak_bypassed_by_high_gamma_pos() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.gamma_pos = 1.0;
        ctx.cw_vs_scw_atr = Some(-5.0);
        assert!(ctx.passes_cw_strength(&c));
    }

    #[test]
    fn spike_expired_at_limit() {
        let mut ctx = base_ctx();
        let cfg = StrategyConfig::default();
        ctx.bars_since_spike = cfg.eff_iv_lookback_bars();
        assert!(ctx.passes_spike_age(&cfg));
        ctx.bars_since_spike = cfg.eff_iv_lookback_bars() + 1;
        assert!(!ctx.passes_spike_age(&cfg));
    }

    #[test]
    fn wall_spread_over_max_fails() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.wall_spread_atr = Some(100.0);
        assert!(!ctx.passes_wall_spread(&c));
    }

    #[test]
    fn wall_spread_none_passes() {
        let c = cfg();
        let mut ctx = base_ctx();
        ctx.wall_spread_atr = None;
        assert!(ctx.passes_wall_spread(&c));
    }

    #[test]
    fn precondition_gates_always_pass() {
        let c = cfg();
        let ctx = base_ctx();
        let cp = compress(&c);
        for gate in [VfGate::NoPw, VfGate::NoSpike, VfGate::NoBaseline, VfGate::IvHigh, VfGate::NoIv] {
            assert!(ctx.check_gate(gate, &c, &cp));
        }
    }

    #[test]
    fn check_gate_matches_individual_methods() {
        let c = cfg();
        let ctx = base_ctx();
        let cp = compress(&c);
        assert_eq!(ctx.check_gate(VfGate::CwWeak, &c, &cp), ctx.passes_cw_strength(&c));
        assert_eq!(ctx.check_gate(VfGate::SpreadWide, &c, &cp), ctx.passes_wall_spread(&c));
        assert_eq!(ctx.check_gate(VfGate::SpikeExpired, &c, &cp), ctx.passes_spike_age(&c));
    }

    #[test]
    fn gamma_pos_basic() {
        assert!((compute_gamma_pos(152.5, 145.0, 160.0) - 0.5).abs() < 0.01);
        assert!((compute_gamma_pos(145.0, 145.0, 160.0)).abs() < 0.01);
        assert!((compute_gamma_pos(160.0, 145.0, 160.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn gamma_pos_clamped() {
        assert_eq!(compute_gamma_pos(200.0, 145.0, 160.0), 1.0);
        assert_eq!(compute_gamma_pos(100.0, 145.0, 160.0), 0.0);
    }

    #[test]
    fn gamma_pos_invalid_walls_fallback() {
        assert_eq!(compute_gamma_pos(150.0, 160.0, 145.0), 0.5); // cw < pw
        assert_eq!(compute_gamma_pos(150.0, 0.0, 155.0), 0.5);   // pw == 0
    }
}
