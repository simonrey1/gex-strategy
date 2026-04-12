use crate::config::StrategyConfig;
use crate::types::{AtrRegimeTsi, Signal};

/// Entry price + [`AtrRegimeTsi`] for VF default bracket math ([`StrategyConfig::compute_stops`]).
#[derive(Debug, Clone, Copy)]
pub struct PlainStopInputs {
    pub entry_price: f64,
    pub regime: AtrRegimeTsi,
}

impl PlainStopInputs {
    #[inline]
    pub const fn new(entry_price: f64, atr: f64, regime_ratio: f64, tsi: f64) -> Self {
        Self {
            entry_price,
            regime: AtrRegimeTsi::new(atr, regime_ratio, tsi),
        }
    }
}

/// Full bracket inputs — same regime triple as [`crate::strategy::slot_sizing::EntryPrepareInputs`] + fill price.
#[derive(Debug, Clone, Copy)]
pub struct StopBracketInputs {
    pub entry_price: f64,
    pub regime: AtrRegimeTsi,
    pub signal: Signal,
    pub tp_cap_atr: f64,
}

impl StopBracketInputs {
    #[inline]
    pub const fn new(
        entry_price: f64,
        atr: f64,
        regime_ratio: f64,
        tsi: f64,
        signal: Signal,
        tp_cap_atr: f64,
    ) -> Self {
        Self {
            entry_price,
            regime: AtrRegimeTsi::new(atr, regime_ratio, tsi),
            signal,
            tp_cap_atr,
        }
    }

    /// VF path with no WB spread cap — matches legacy [`StrategyConfig::compute_stops`].
    #[inline]
    pub const fn vanna_flip_from_plain(plain: PlainStopInputs) -> Self {
        Self {
            entry_price: plain.entry_price,
            regime: plain.regime,
            signal: Signal::LongVannaFlip,
            tp_cap_atr: 0.0,
        }
    }
}

/// SL/TP computation (shared by live and backtest, all signal types).
impl StrategyConfig {
    /// Compute SL/TP using VF defaults (no signal-specific TP routing).
    pub fn compute_stops(&self, plain: &PlainStopInputs) -> Option<(f64, f64)> {
        self.compute_stops_for(&StopBracketInputs::vanna_flip_from_plain(*plain))
    }

    /// Compute SL/TP with signal-specific TP routing.
    pub fn compute_stops_for(&self, b: &StopBracketInputs) -> Option<(f64, f64)> {
        let mut sl_atr = self.bracket_sl_atr();
        if self.sl_tsi_adapt > 0.0 && b.regime.tsi.is_finite() {
            let tsi_norm = (b.regime.tsi / 50.0).clamp(-1.0, 1.0);
            let adapt = self.sl_tsi_adapt / b.regime.atr_regime_ratio.max(1.0).sqrt();
            sl_atr += -tsi_norm * adapt;
        }

        let raw_tp = match b.signal {
            Signal::LongWallBounce => {
                if self.wb_tp_spread_mult > 0.0 && b.tp_cap_atr > 0.0 {
                    b.tp_cap_atr * self.wb_tp_spread_mult
                } else {
                    self.bracket_tp_atr()
                }
            }
            Signal::LongVannaFlip | Signal::Flat => self.bracket_tp_atr(),
        };

        let tp_atr = raw_tp * b.regime.atr_regime_ratio.max(1.0).sqrt();
        let sl = crate::types::round_cents(b.entry_price - b.regime.atr * sl_atr);
        let tp = crate::types::round_cents(b.entry_price + b.regime.atr * tp_atr);
        Some((sl, tp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_vf_basic() {
        let cfg = StrategyConfig::default();
        let plain = PlainStopInputs::new(100.0, 2.0, 1.0, 0.0);
        let (sl, tp) = cfg.compute_stops(&plain).unwrap();
        assert!(sl < 100.0, "SL should be below entry");
        assert!(tp > 100.0, "TP should be above entry");
        let expected_sl = crate::types::round_cents(100.0 - 2.0 * cfg.bracket_sl_atr());
        assert!((sl - expected_sl).abs() < 0.01);
    }

    #[test]
    fn stops_wb_uses_tp_cap() {
        let cfg = StrategyConfig::default();
        let base = StopBracketInputs::new(100.0, 2.0, 1.0, 0.0, Signal::LongVannaFlip, 0.0);
        let (_, tp_vf) = cfg.compute_stops_for(&base).unwrap();
        let wb = StopBracketInputs::new(100.0, 2.0, 1.0, 0.0, Signal::LongWallBounce, 5.0);
        let (_, tp_wb) = cfg.compute_stops_for(&wb).unwrap();
        if cfg.wb_tp_spread_mult > 0.0 {
            assert!(tp_wb != tp_vf, "WB TP should differ from VF when tp_cap_atr is set");
        }
    }
}
