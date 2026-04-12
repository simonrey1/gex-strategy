use crate::config::StrategyConfig;
use crate::config::strategy::{HALF, WB_MAX_BARS_SINCE_ABOVE};
use crate::strategy::signals::{SignalState, WallDiffInputs, WB_MAX_PW_DIST_ATR};
use crate::types::{Rejection, Signal, TradeSignal, VfGate};

use super::guards::*;
use super::EntryTradeSignalBuild;
use super::vf_gates::{VfCompressParams, VfGateCtx};
use super::BarCtx;

#[inline]
fn rej_first_to_string(v: Vec<Rejection>) -> String {
    v.into_iter().next().map_or_else(|| "unknown".into(), |r| r.to_string())
}

impl SignalState {
    pub fn spike_alpha(&self, config: &StrategyConfig) -> Result<f64, Rejection> {
        if self.iv_baseline_ema <= 0.0 {
            return Err(Rejection::plain(VfGate::NoBaseline));
        }
        let raw = (self.iv_spike_level / self.iv_baseline_ema - 1.0).max(0.0);
        Ok((raw / (config.iv_spike_mult - 1.0).max(0.01)).min(1.0))
    }
}

// ── VF gate context ─────────────────────────────────────────────────────────

/// Bundles BarCtx + VF-specific precomputed fields.
pub struct VfCtx<'a> {
    pub ctx: &'a BarCtx<'a>,
    pw: f64,
    ncw: f64,
}

impl<'a> VfCtx<'a> {
    #[inline]
    fn st(&self) -> &'a SignalState {
        self.ctx.state
    }

    fn gate_ctx(&self) -> VfGateCtx {
        VfGateCtx::from(self.ctx)
    }

    fn gate_atr_pct(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        if !g.passes_atr_min(self.ctx.cfg) {
            return Err(Rejection::new(VfGate::AtrPct, format!("{:.2}<{:.2}", g.atr_pct, self.ctx.cfg.vf_min_atr_pct)));
        }
        if !g.passes_atr_max(self.ctx.cfg) {
            return Err(Rejection::new(VfGate::AtrPct, format!("{:.2}>={:.2}", g.atr_pct, self.ctx.cfg.vf_max_atr_pct)));
        }
        if !g.passes_slow_atr_max(self.ctx.cfg) {
            return Err(Rejection::new(VfGate::SlowAtr, format!("{:.2}>={:.2}", g.slow_atr_pct, self.ctx.cfg.vf_max_slow_atr_pct)));
        }
        Ok(())
    }

    fn gate_dead_zone(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        let dz_lo = self.ctx.cfg.dead_zone_lo();
        let dz_hi = self.ctx.cfg.dead_zone_hi();
        if dz_lo < dz_hi && g.regime.tsi >= dz_lo && g.regime.tsi < dz_hi {
            let ncw_gap = if self.ncw > 0.0 && self.ctx.atr() > 0.0 {
                (self.ncw - self.ctx.bar.close) / self.ctx.atr()
            } else {
                0.0
            };
            let bad_ncw = ncw_gap < self.ctx.cfg.dead_zone_min_ncw_atr();
            let bad_adx = !g.passes_dead_zone_adx(self.ctx.cfg);
            if bad_ncw || bad_adx {
                return Err(Rejection::new(VfGate::TsiDead, format!(
                    "tsi={:.0},ncw={ncw_gap:.1},atm={:.2},adx={:.0}",
                    g.regime.tsi, self.ctx.atm_gamma_dominance(), g.regime.adx,
                )));
            }
        }
        Ok(())
    }

    fn gate_spread(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        if !g.passes_wall_spread(self.ctx.cfg) {
            return Err(Rejection::new(VfGate::SpreadWide, format!("{:.1}>{:.1}",
                self.st().spread_atr(),
                self.ctx.cfg.vf_max_wall_spread_atr,
            )));
        }
        Ok(())
    }

    fn gate_cw_weak(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        let s = self.st();
        if self.ncw > 0.0 && s.spread_call_wall() > 0.0
            && !g.passes_cw_strength(self.ctx.cfg)
            && s.cw_below_smooth_bars >= self.ctx.cfg.vf_cw_scw_persist_bars
        {
            return Err(Rejection::new(VfGate::CwWeak, format!(
                "ncw={:.0},scw={:.0},diff={:.2}atr,bars={},thr={:.2}",
                self.ncw, s.spread_call_wall(), s.cw_scw_atr(),
                s.cw_below_smooth_bars, HALF,
            )));
        }
        Ok(())
    }

    fn gate_pw_weak(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        if self.ctx.cfg.vf_min_pw_spw_atr > -900.0 && self.ctx.narrow_pw() > 0.0 && self.pw > 0.0
            && !g.passes_pw_strength(self.ctx.cfg)
        {
            let thr = self.ctx.cfg.eff_pw_spw_threshold(g.regime.atr_regime_ratio);
            return Err(Rejection::new(VfGate::PwWeak, format!(
                "npw={:.0},spw={:.0},diff={:.2}atr,thr={thr:.2},base={:.2},ratio={:.2}",
                self.ctx.narrow_pw(), self.pw, self.st().pw_spw_atr(),
                self.ctx.cfg.vf_min_pw_spw_atr, g.regime.atr_regime_ratio,
            )));
        }
        Ok(())
    }

    fn gate_gex_norm(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        if !g.passes_gex_norm(self.ctx.cfg) {
            let gn = self.st().gex_norm(self.ctx.net_gex());
            let thr = self.ctx.cfg.eff_gex_norm_threshold(g.regime.atr_regime_ratio);
            return Err(Rejection::new(VfGate::GexNorm, format!("{gn:.2}>{thr:.2}")));
        }
        Ok(())
    }

    fn gate_dealer_flow(&self, g: &VfGateCtx) -> Result<(), Rejection> {
        let cfg = self.ctx.cfg;
        if !g.passes_rally_cap(cfg) {
            return Err(Rejection::new(VfGate::RallyCap,
                format!("ret={:.2}>{:.2},bars={}", g.cum_return_atr, cfg.vf_max_rally_atr, g.bars_since_spike)));
        }
        if !g.passes_spike_vanna(cfg) {
            return Err(Rejection::new(VfGate::SpikeVanna,
                format!("{:.0}<{:.0}", g.spike_vanna, cfg.vf_min_spike_vanna)));
        }
        if !g.passes_spike_gamma_tilt(cfg) {
            return Err(Rejection::new(VfGate::SpikeGammaTilt,
                format!("{:.2}<{:.2}", g.spike_gamma_tilt, cfg.vf_min_spike_gamma_tilt)));
        }
        if !g.passes_pw_drift(cfg) {
            return Err(Rejection::new(VfGate::PwDrift,
                format!("{:.2}<{:.2}", g.pw_drift_atr, cfg.vf_min_pw_drift_atr)));
        }
        if !g.passes_gamma_tilt(cfg) {
            return Err(Rejection::new(VfGate::GammaTilt,
                format!("{:.2}<{:.2}", g.gamma_tilt, cfg.vf_min_gamma_tilt)));
        }
        Ok(())
    }

    fn gate_iv(&self, g: &VfGateCtx) -> Result<f64, Rejection> {
        if !g.passes_tsi_max(self.ctx.cfg) {
            let eff = StrategyConfig::eff_vf_max_tsi(g.regime.atr_regime_ratio);
            return Err(Rejection::new(VfGate::Tsi, format!("{:.1}>{eff:.1}r={:.2}", g.regime.tsi, g.regime.atr_regime_ratio)));
        }
        if !self.st().iv_scan_eligible(self.ctx.gex, self.ctx.cfg, self.ctx.ind.tsi) {
            if !g.passes_spike_age(self.ctx.cfg) {
                return Err(Rejection::plain(VfGate::SpikeExpired));
            }
            return Err(Rejection::plain(VfGate::IvHigh));
        }
        self.ctx.atm_put_iv_opt().ok_or_else(|| Rejection::plain(VfGate::NoIv))
    }

    fn gate_iv_compress(&self, g: &VfGateCtx, iv_now: f64) -> Result<(), Rejection> {
        if self.st().iv_spike_level > 0.0 {
            let c = self.st().compress_ratio(iv_now);
            let compress = VfCompressParams::new(c, self.ctx.cfg);
            if !g.passes_iv_compress(&compress) {
                return Err(Rejection::new(VfGate::IvCompress, format!("{c:.3}>{:.3},tsi={:.0}",
                    HALF, g.regime.tsi)));
            }
        }
        Ok(())
    }

    /// Run all gates. Short-circuits on first failure when `collect_all` is false.
    pub fn evaluate(&self, collect_all: bool) -> Result<f64, Vec<Rejection>> {
        let g = self.gate_ctx();
        let mut fails: Vec<Rejection> = Vec::new();

        macro_rules! check {
            ($gate:expr) => {
                if let Err(e) = $gate {
                    if !collect_all { return Err(vec![e]); }
                    fails.push(e);
                }
            };
        }

        check!(self.gate_atr_pct(&g));
        check!(self.gate_dead_zone(&g));
        check!(self.gate_spread(&g));
        check!(self.gate_cw_weak(&g));
        check!(self.gate_pw_weak(&g));
        check!(self.gate_gex_norm(&g));
        check!(self.gate_dealer_flow(&g));

        let iv_now = match self.gate_iv(&g) {
            Ok(iv) => {
                check!(self.gate_iv_compress(&g, iv));
                Some(iv)
            }
            Err(e) => {
                if !collect_all { return Err(vec![e]); }
                fails.push(e);
                None
            }
        };

        if !fails.is_empty() {
            return Err(fails);
        }
        Ok(iv_now.unwrap())
    }
}

/// Build a VfCtx after checking preconditions. Returns Err on precondition failure.
pub fn vf_ctx<'a>(bctx: &'a BarCtx<'a>) -> Result<VfCtx<'a>, Vec<Rejection>> {
    let s = bctx.state;
    let w = s.smoothed_walls();
    if w.pw <= 0.0 { return Err(vec![Rejection::plain(VfGate::NoPw)]); }
    if !s.has_active_spike() { return Err(vec![Rejection::plain(VfGate::NoSpike)]); }
    match s.spike_alpha(bctx.cfg) {
        Ok(a) if a > 0.0 => {}
        Ok(_) => return Err(vec![Rejection::plain(VfGate::NoSpike)]),
        Err(e) => return Err(vec![e]),
    }
    Ok(VfCtx { ctx: bctx, pw: w.pw, ncw: bctx.narrow_cw() })
}


// ── Public API ──────────────────────────────────────────────────────────────

/// VannaFlip (IV spike recovery) entry — spike path only.
pub(super) fn try_vanna_flip(bctx: &BarCtx) -> Result<TradeSignal, String> {
    let ctx = vf_ctx(bctx).map_err(rej_first_to_string)?;
    let iv_now = ctx.evaluate(false).map_err(rej_first_to_string)?;
    let s = bctx.state;
    let SmoothedWalls { pw, cw } = s.smoothed_walls();
    let bars = s.bars_since_spike();
    let compress_pct = s.compress_pct(iv_now);
    let reason = format!(
        "{} pw=${pw:.0} cw=${cw:.0} iv={iv_now:.3}/{:.3}({compress_pct:.0}%) bars={bars} tsi={:.0}",
        Signal::LongVannaFlip.short_name(),
        s.iv_spike_level, bctx.tsi(),
    );

    Ok(EntryTradeSignalBuild::new(bctx.bar, Signal::LongVannaFlip, reason).into_trade_signal())
}

// ── WallBounce ──────────────────────────────────────────────────────────────

/// Calm-path WallBounce: zone dwell near narrow put wall with no active spike.
pub(super) fn try_wall_bounce_calm(bctx: &BarCtx) -> Result<TradeSignal, String> {
    if !bctx.ticker.is_wb_enabled() { return Err("wb_disabled".into()); }
    let s = bctx.state;
    if s.has_active_spike() { return Err("wb_spike_active".into()); }

    let (pw, cw) = bctx.wb_wall_pair();
    if pw <= 0.0 { return Err("wb_no_pw".into()); }
    if cw <= 0.0 || cw <= pw { return Err("wb_no_cw".into()); }

    SpreadBandInputs::new(
        WallDiffInputs::wb_trail_band_spread_atr(pw, cw, bctx.atr()),
        bctx.cfg.wb_min_wall_spread_atr,
        bctx.cfg.wb_max_wall_spread_atr(),
        "wb",
    )
    .validate()?;

    if bctx.bar.close < pw { return Err(format!("wb_below_pw(${pw:.0})")); }
    let dist_atr = (bctx.bar.close - pw) / bctx.atr();
    if dist_atr > WB_MAX_PW_DIST_ATR {
        return Err(format!("wb_too_far({dist_atr:.1}>{WB_MAX_PW_DIST_ATR:.1})"));
    }

    if WB_MAX_BARS_SINCE_ABOVE < 900 {
        let bsa = s.entry_pw_zone.bars_since_above;
        let max_bsa = WB_MAX_BARS_SINCE_ABOVE;
        if !(0..=max_bsa).contains(&bsa) {
            return Err(format!("wb_not_from_above(bsa={bsa})"));
        }
    }

    if s.entry_pw_zone.zone_score < bctx.cfg.wb_min_zone_score {
        return Err(format!("wb_zscore({:.1}<{:.1})", s.entry_pw_zone.zone_score, bctx.cfg.wb_min_zone_score));
    }

    if bctx.bar.close < bctx.trend_ema() {
        return Err("wb_below_trend".into());
    }

    let ema_gap_pct = (bctx.ema_fast() - bctx.ema_slow()) / bctx.ema_slow() * 100.0;
    if ema_gap_pct < bctx.cfg.wb_min_ema_gap_pct {
        return Err(format!("wb_ema_gap({ema_gap_pct:.3}<{:.3})", bctx.cfg.wb_min_ema_gap_pct));
    }
    if !bctx.tsi_bullish() {
        return Err("wb_tsi_bearish".into());
    }
    if bctx.tsi() < bctx.cfg.wb_min_tsi {
        return Err(format!("wb_tsi_low({:.0}<{:.0})", bctx.tsi(), bctx.cfg.wb_min_tsi));
    }

    let reason = format!(
        "{} pw=${pw:.0} cw=${cw:.0} dist={dist_atr:.1}atr zscore={:.1}",
        Signal::LongWallBounce.short_name(), s.entry_pw_zone.zone_score,
    );

    Ok(EntryTradeSignalBuild::new(bctx.bar, Signal::LongWallBounce, reason).into_trade_signal())
}
