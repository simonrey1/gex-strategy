use crate::types::{GexProfile, ToF64};

/// γ×OI-weighted EMA smoother for GEX wall strikes.
///
/// Strong walls (high γ×OI) move the EMA faster; weak walls barely nudge it.
/// Produces smooth, conviction-weighted support/resistance levels.
///
/// `window` = base EMA half-life in strategy bars. 0 = disabled (passthrough).
pub struct WallSmoother {
    window: usize,
    alpha: f64,
    spread_alpha: f64,
    pw: f64,
    cw: f64,
    /// Pure-EMA smoothed walls (no γ×OI weighting) for the spread gate.
    spread_pw: f64,
    spread_cw: f64,
    /// EMA of the highest narrow put wall strike (closest to spot among top 2).
    highest_pw: f64,
    /// EMA of the lowest narrow call wall strike (closest to spot among top 2).
    lowest_cw: f64,
}

/// Raw wall strike + γ×OI for [`WallSmoother::update_pw`] / [`WallSmoother::update_cw`].
#[derive(Clone, Copy, Debug)]
pub struct WallSmootherWeightedStep {
    pub raw: f64,
    pub gamma_oi: f64,
}

impl WallSmootherWeightedStep {
    #[inline]
    pub const fn new(raw: f64, gamma_oi: f64) -> Self {
        Self { raw, gamma_oi }
    }
}

/// Packed put/call steps + concentrations from [`GexProfile::wall_smoother_inputs`].
#[derive(Clone, Copy, Debug)]
pub struct WallSmootherGexBundle {
    pub pw: WallSmootherWeightedStep,
    pub cw: WallSmootherWeightedStep,
    pub pw_conc: f64,
    pub cw_conc: f64,
}

/// Narrow + spread smoothed strikes and γ concentrations (mirrors [`WallSmoother`] + bundle after each GEX bar).
#[derive(Clone, Copy, Debug)]
pub struct SmoothedWallLevels {
    pub smoothed_pw: f64,
    pub smoothed_cw: f64,
    pub spread_pw: f64,
    pub spread_cw: f64,
    pub pw_conc: f64,
    pub cw_conc: f64,
}

impl Default for SmoothedWallLevels {
    fn default() -> Self {
        Self {
            smoothed_pw: 0.0,
            smoothed_cw: 0.0,
            spread_pw: 0.0,
            spread_cw: 0.0,
            pw_conc: f64::NAN,
            cw_conc: f64::NAN,
        }
    }
}

impl SmoothedWallLevels {
    /// Snapshot after [`WallSmoother::update_from_gex`] (same values previously copied field-by-field into [`crate::strategy::signals::SignalState`]).
    #[inline]
    pub fn from_smoother(smoother: &WallSmoother, bundle: &WallSmootherGexBundle) -> Self {
        Self {
            smoothed_pw: smoother.pw(),
            smoothed_cw: smoother.cw(),
            spread_pw: smoother.spread_pw(),
            spread_cw: smoother.spread_cw(),
            pw_conc: bundle.pw_conc,
            cw_conc: bundle.cw_conc,
        }
    }

}

#[inline]
fn gex_wall_bundle(gex: &GexProfile) -> WallSmootherGexBundle {
    let ((pw_s, pw_g), (cw_s, cw_g), pw_conc, cw_conc) = gex.wall_smoother_inputs();
    WallSmootherGexBundle {
        pw: WallSmootherWeightedStep::new(pw_s, pw_g),
        cw: WallSmootherWeightedStep::new(cw_s, cw_g),
        pw_conc,
        cw_conc,
    }
}

fn halflife_alpha(window: usize) -> f64 {
    if window == 0 {
        1.0
    } else {
        1.0 - 2.0_f64.powf(-1.0 / window.to_f64())
    }
}

impl WallSmoother {
    pub fn new(window: usize) -> Self {
        Self::with_spread_halflife(window, window)
    }

    pub fn with_spread_halflife(window: usize, spread_halflife: usize) -> Self {
        Self {
            window,
            alpha: halflife_alpha(window),
            spread_alpha: halflife_alpha(if window == 0 { 0 } else { spread_halflife }),
            pw: 0.0, cw: 0.0, spread_pw: 0.0, spread_cw: 0.0, highest_pw: 0.0, lowest_cw: 0.0,
        }
    }

    pub fn disabled() -> Self {
        Self::new(0)
    }

    pub fn is_enabled(&self) -> bool {
        self.window > 0
    }

    pub fn pw(&self) -> f64 { self.pw }
    pub fn cw(&self) -> f64 { self.cw }
    pub fn spread_pw(&self) -> f64 { self.spread_pw }
    pub fn spread_cw(&self) -> f64 { self.spread_cw }
    pub fn smoothed_highest_pw(&self) -> f64 { self.highest_pw }
    pub fn smoothed_lowest_cw(&self) -> f64 { self.lowest_cw }

    pub fn update_pw(&mut self, step: WallSmootherWeightedStep) -> f64 {
        self.pw = self.smooth_weighted(self.pw, step.raw, step.gamma_oi);
        self.spread_pw = self.smooth_ema(self.spread_pw, step.raw);
        self.pw
    }

    pub fn update_cw(&mut self, step: WallSmootherWeightedStep) -> f64 {
        self.cw = self.smooth_weighted(self.cw, step.raw, step.gamma_oi);
        self.spread_cw = self.smooth_ema(self.spread_cw, step.raw);
        self.cw
    }

    /// Smooth the highest narrow put wall strike (simple EMA, no γ×OI weighting).
    pub fn update_highest_pw(&mut self, raw: f64) -> f64 {
        self.highest_pw = self.smooth_ema(self.highest_pw, raw);
        self.highest_pw
    }

    /// Smooth the lowest narrow call wall strike (simple EMA, no γ×OI weighting).
    pub fn update_lowest_cw(&mut self, raw: f64) -> f64 {
        self.lowest_cw = self.smooth_ema(self.lowest_cw, raw);
        self.lowest_cw
    }

    /// Narrow put/call wall strikes from `gex` (γ×OI-weighted + spread EMAs), plus top-2
    /// highest PW / lowest CW EMAs. Returns concentrations for signal state.
    pub fn update_from_gex(&mut self, gex: &GexProfile) -> WallSmootherGexBundle {
        let w = gex_wall_bundle(gex);
        self.update_pw(w.pw);
        self.update_cw(w.cw);
        self.update_highest_pw(gex.top2_highest_pw());
        self.update_lowest_cw(gex.top2_lowest_cw());
        w
    }

    fn smooth_weighted(&self, current: f64, raw: f64, gamma_oi: f64) -> f64 {
        if raw <= 0.0 { return current; }
        if !self.is_enabled() || current <= 0.0 { return raw; }
        let strength = (gamma_oi.abs() / 1e6).clamp(0.1, 3.0);
        let effective_alpha = (self.alpha * strength).min(1.0);
        current + effective_alpha * (raw - current)
    }

    fn smooth_ema(&self, current: f64, raw: f64) -> f64 {
        if raw <= 0.0 { return current; }
        if !self.is_enabled() || current <= 0.0 { return raw; }
        current + self.spread_alpha * (raw - current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_passthrough() {
        let mut s = WallSmoother::new(0);
        assert!(!s.is_enabled());
        assert_eq!(s.update_pw(WallSmootherWeightedStep::new(100.0, 1e6)), 100.0);
        assert_eq!(s.update_pw(WallSmootherWeightedStep::new(90.0, 1e6)), 90.0);
    }

    #[test]
    fn strong_gamma_moves_faster() {
        let mut s1 = WallSmoother::new(10);
        let mut s2 = WallSmoother::new(10);
        s1.update_pw(WallSmootherWeightedStep::new(100.0, 1e6));
        s2.update_pw(WallSmootherWeightedStep::new(100.0, 1e6));
        let after_strong = s1.update_pw(WallSmootherWeightedStep::new(110.0, 5e6));
        let after_weak = s2.update_pw(WallSmootherWeightedStep::new(110.0, 0.1e6));
        assert!(after_strong > after_weak, "strong gamma should move EMA faster: {after_strong} vs {after_weak}");
    }

    #[test]
    fn skips_zero_raw() {
        let mut s = WallSmoother::new(10);
        s.update_pw(WallSmootherWeightedStep::new(100.0, 1e6));
        assert_eq!(s.update_pw(WallSmootherWeightedStep::new(0.0, 1e6)), 100.0);
    }

    #[test]
    fn convergence() {
        let mut s = WallSmoother::new(10);
        s.update_pw(WallSmootherWeightedStep::new(100.0, 1e6));
        for _ in 0..100 {
            s.update_pw(WallSmootherWeightedStep::new(50.0, 1e6));
        }
        assert!((s.pw() - 50.0).abs() < 1.0, "should converge to 50: {}", s.pw());
    }
}
