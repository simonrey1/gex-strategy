use crate::config::StrategyConfig;
use crate::config::strategy::{HURST_MIN_PERSIST_BARS, HURST_WINDOW, SOFT_TRAIL_HURST_MULT};
use crate::strategy::hurst::HurstTracker;
use crate::strategy::trail_fields::TrailFields;
use crate::types::{BarPriceAtr, BarVolRegime};

/// Outcome of a wall-trailing SL check on a strategy bar.
pub enum WallTrailOutcome {
    Unchanged,
    Ratcheted { old_sl: f64, new_sl: f64 },
    /// Early TP triggered by Hurst exhaustion before wall trail activates.
    EarlyTp,
}

/// Smoothed put wall + [`BarVolRegime`] + normalized GEX for [`check_trail`].
#[derive(Debug, Clone, Copy)]
pub struct TrailCheckInputs {
    pub trail_pw: f64,
    pub vol: BarVolRegime,
    /// net_gex / EMA(|net_gex|). Positive = dealers long gamma (dampen).
    pub gex_norm: f64,
}

impl TrailCheckInputs {
    #[inline]
    pub const fn new(trail_pw: f64, vol: BarVolRegime, gex_norm: f64) -> Self {
        Self { trail_pw, vol, gex_norm }
    }
}

/// Put wall + entry + [`BarPriceAtr`] for [`wall_trail_sl`] (structural ratchet step).
#[derive(Debug, Clone, Copy)]
pub struct WallTrailRatchetInputs {
    pub put_wall: f64,
    pub entry_price: f64,
    pub price_atr: BarPriceAtr,
}

impl WallTrailRatchetInputs {}


/// Full trail check: wall trail + hurst exhaustion + early TP.
/// Shared by backtest positions and IV scan — single source of truth.
pub fn check_trail(
    tf: TrailFields<'_>,
    inputs: TrailCheckInputs,
    config: &StrategyConfig,
    hurst: &mut HurstTracker,
) -> WallTrailOutcome {
    use crate::types::Signal;

    hurst.push(inputs.vol.close);

    if inputs.vol.close > *tf.highest_close {
        *tf.highest_close = inputs.vol.close;
    }

    let old_sl = *tf.stop_loss;
    let is_wb = matches!(tf.signal, Signal::LongWallBounce);

    // Early Hurst TP (soft trail zone)
    if inputs.vol.atr > 0.0 {
        let runup_atr = (*tf.highest_close - tf.entry_price) / inputs.vol.atr;
        let soft_activate = config.soft_trail_activate_atr();
        let wall_activate = config.wall_trail_activate_atr();
        if runup_atr >= soft_activate && runup_atr < wall_activate {
            if let Some(h) = hurst.hurst_max(&[48, HURST_WINDOW]) {
                let thresh = config.eff_hurst_exhaust_threshold(inputs.vol.atr_regime_ratio)
                    * SOFT_TRAIL_HURST_MULT;
                if h < thresh {
                    return WallTrailOutcome::EarlyTp;
                }
            }
        }
    }

    // WB: skip structural wall trail — put wall movement is not the exit signal.
    if !is_wb {
        wall_trail_sl(
            tf.stop_loss,
            tf.highest_put_wall,
            WallTrailRatchetInputs { put_wall: inputs.trail_pw, entry_price: tf.entry_price, price_atr: BarPriceAtr::new(inputs.vol.close, inputs.vol.atr) },
            config,
        );
    }

    // Hurst exhaustion trailing
    if config.hurst_exhaust_threshold > 0.0 && inputs.vol.atr > 0.0 {
        let sl_ratio = 5.0_f64 / config.bracket_sl_atr();
        let effective_min_gain = config.hurst_min_gain_atr() * sl_ratio;
        let gain_atr = (inputs.vol.close - tf.entry_price) / inputs.vol.atr;
        if gain_atr >= effective_min_gain {
            if let Some(h) = hurst.hurst_max(&[48, HURST_WINDOW]) {
                let adaptive_thresh = config.eff_hurst_exhaust_threshold(inputs.vol.atr_regime_ratio);
                if h < adaptive_thresh {
                    *tf.hurst_exhaust_bars += 1;
                } else {
                    *tf.hurst_exhaust_bars = 0;
                }
                if *tf.hurst_exhaust_bars >= HURST_MIN_PERSIST_BARS {
                    let trail_sl = crate::types::round_cents(
                        *tf.highest_close - config.hurst_trail_atr() * inputs.vol.atr,
                    );
                    if trail_sl > *tf.stop_loss {
                        *tf.stop_loss = trail_sl;
                    }
                }
            }
        }
    }

    // Profit floor trail: loose trailing once runup exceeds threshold
    if config.profit_floor_activate_atr > 0.0 && inputs.vol.atr > 0.0 {
        let runup_atr = (*tf.highest_close - tf.entry_price) / inputs.vol.atr;
        if runup_atr >= config.profit_floor_activate_atr {
            let floor_sl = crate::types::round_cents(
                *tf.highest_close - config.profit_floor_trail_atr * inputs.vol.atr,
            );
            if floor_sl > *tf.stop_loss {
                *tf.stop_loss = floor_sl;
            }
        }
    }

    // TP proximity trail: applies to both VF and WB
    if inputs.vol.atr > 0.0 && tf.tp > tf.entry_price {
        let tp_range = tf.tp - tf.entry_price;
        let progress = (*tf.highest_close - tf.entry_price) / tp_range;
        if progress >= config.tp_proximity_trigger() {
            let cushion = config.wall_trail_cushion_atr();
            let trail_sl = crate::types::round_cents(*tf.highest_close - cushion * inputs.vol.atr);
            if trail_sl > *tf.stop_loss {
                *tf.stop_loss = trail_sl;
            }
        }
    }

    if *tf.stop_loss > old_sl {
        WallTrailOutcome::Ratcheted { old_sl, new_sl: *tf.stop_loss }
    } else {
        WallTrailOutcome::Unchanged
    }
}

/// Update SL based on structural put wall movement. Called on each strategy bar.
///
/// Scans all put walls and picks the highest one below price (nearest support).
/// If that wall is higher than the previous highest, ratchet SL up to
/// `put_wall - cushion * ATR`. The SL never moves down.
pub fn wall_trail_sl(
    stop_loss: &mut f64,
    highest_wall: &mut f64,
    inputs: WallTrailRatchetInputs,
    config: &StrategyConfig,
) {
    if inputs.price_atr.atr <= 0.0 || inputs.put_wall <= 0.0 { return; }
    if (inputs.price_atr.close - inputs.entry_price) / inputs.price_atr.atr < config.wall_trail_activate_atr() { return; }

    if inputs.put_wall > *highest_wall && inputs.price_atr.close > inputs.put_wall {
        *highest_wall = inputs.put_wall;
    }

    if *highest_wall <= 0.0 { return; }

    let wall_sl = crate::types::round_cents(
        *highest_wall - config.wall_trail_cushion_atr() * inputs.price_atr.atr,
    );

    if wall_sl > *stop_loss {
        *stop_loss = wall_sl;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StrategyConfig;

    #[test]
    fn wall_trail_not_activated_below_threshold() {
        let cfg = StrategyConfig::default();
        let mut sl = 90.0;
        let mut hw = 0.0;
        wall_trail_sl(&mut sl, &mut hw, WallTrailRatchetInputs { put_wall: 93.0, entry_price: 90.0, price_atr: BarPriceAtr::new(95.0, 1.0) }, &cfg);
        assert!((sl - 90.0).abs() < 0.01, "SL should not change below activate threshold");
    }

    #[test]
    fn wall_trail_ratchets_up() {
        let mut cfg = StrategyConfig::default();
        cfg.exit_width_atr = 2.0;
        let mut sl = 90.0;
        let mut hw = 0.0;
        wall_trail_sl(&mut sl, &mut hw, WallTrailRatchetInputs { put_wall: 107.0, entry_price: 100.0, price_atr: BarPriceAtr::new(110.0, 1.0) }, &cfg);
        assert!(hw > 0.0, "highest wall should be set");
        assert!(sl > 90.0, "SL should ratchet up");
        let expected = crate::types::round_cents(107.0 - cfg.wall_trail_cushion_atr() * 1.0);
        assert!((sl - expected).abs() < 0.01);
    }

    #[test]
    fn wall_trail_never_moves_down() {
        let mut cfg = StrategyConfig::default();
        cfg.exit_width_atr = 2.0;
        let mut sl = 106.5;
        let mut hw = 107.0;
        wall_trail_sl(&mut sl, &mut hw, WallTrailRatchetInputs { put_wall: 104.0, entry_price: 100.0, price_atr: BarPriceAtr::new(110.0, 1.0) }, &cfg);
        assert!((sl - 106.5).abs() < 0.01, "SL should not move down: got {sl}");
    }

    #[test]
    fn wall_trail_zero_atr_noop() {
        let cfg = StrategyConfig::default();
        let mut sl = 90.0;
        let mut hw = 0.0;
        wall_trail_sl(&mut sl, &mut hw, WallTrailRatchetInputs { put_wall: 95.0, entry_price: 90.0, price_atr: BarPriceAtr::new(100.0, 0.0) }, &cfg);
        assert!((sl - 90.0).abs() < 0.01);
        wall_trail_sl(&mut sl, &mut hw, WallTrailRatchetInputs { put_wall: 95.0, entry_price: 90.0, price_atr: BarPriceAtr::new(100.0, 0.0) }, &cfg);
        assert!((sl - 90.0).abs() < 0.01);
    }
}
