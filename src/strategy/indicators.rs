use crate::config::strategy::*;
use crate::config::StrategyConfig;
use crate::types::{BarVolRegime, OhlcBar};

/// Compile-time `f64` mirrors of fixed lengths — avoids runtime `to_f64()` on every bar.
const ATR_LENGTH_F: f64 = ATR_LENGTH as f64;
const DI_LENGTH_F: f64 = DI_LENGTH as f64;
const EMA_FAST_LEN_F: f64 = EMA_FAST_LEN as f64;
const EMA_SLOW_LEN_F: f64 = EMA_SLOW_LEN as f64;
const IV_LOOKBACK_BARS_F: f64 = IV_LOOKBACK_BARS as f64;
const ATR_REGIME_EMA_LEN_F: f64 = ATR_REGIME_EMA_LEN as f64;
const TSI_LONG_LEN_F: f64 = TSI_LONG_LENGTH as f64;
const TSI_SHORT_LEN_F: f64 = TSI_SHORT_LENGTH as f64;
const TSI_SIGNAL_LEN_F: f64 = TSI_SIGNAL_LENGTH as f64;

#[derive(Debug, Clone, Copy)]
pub struct IndicatorValues {
    pub atr: f64,
    pub ema_fast: f64,
    pub ema_slow: f64,
    pub adx: f64,
    pub trend_ema: f64,
    pub tsi: f64,
    pub tsi_bullish: bool,
    /// ATR / EMA(ATR). > 1.0 means elevated volatility vs recent history.
    pub atr_regime_ratio: f64,
    /// EMA(ATR, 250 bars) — slow ATR baseline for regime detection.
    pub atr_regime_ema: f64,
}

impl IndicatorValues {
    /// Close + ATR + regime for wall trail / adaptive thresholds (same fields as [`BarVolRegime`]).
    #[inline]
    pub fn bar_vol_regime(&self, close: f64) -> BarVolRegime {
        BarVolRegime::new(close, self.atr, self.atr_regime_ratio)
    }

    /// ATR + TSI for [`crate::strategy::entry_candidate_data::EntryCandidateData`] / slot sizing.
    #[inline]
    pub fn entry_atr_tsi(&self) -> crate::types::EntryAtrTsi {
        crate::types::EntryAtrTsi::new(self.atr, self.tsi)
    }
}

pub struct IncrementalIndicators {
    count: usize,

    prev_close: f64,
    prev_high: f64,
    prev_low: f64,

    // Wilder-smoothed ATR
    atr: f64,
    atr_period: usize,
    atr_tr_sum: f64,

    // EMA fast/slow
    ema_fast: f64,
    ema_slow: f64,
    ema_fast_k: f64,
    ema_slow_k: f64,
    close_sum_fast: f64,
    close_sum_slow: f64,

    // Multi-day trend EMA
    trend_ema: f64,
    trend_ema_k: f64,
    trend_ema_len: usize,
    close_sum_trend: f64,

    // ADX via Wilder smoothing
    smooth_plus_dm: f64,
    smooth_minus_dm: f64,
    smooth_tr: f64,
    adx: f64,
    di_period: usize,
    di_tr_sum: f64,
    plus_dm_sum: f64,
    minus_dm_sum: f64,
    dx_sum: f64,
    dx_count: usize,

    // ATR regime: slow EMA of ATR for volatility regime detection
    atr_regime_ema: f64,
    atr_regime_ema_k: f64,
    atr_regime_bars: usize,
    atr_regime_sum: f64,
    atr_regime_len: usize,

    // Volume regime: slow EMA of volume for volume regime detection
    vol_regime_ema: f64,
    vol_regime_ema_k: f64,
    vol_regime_bars: usize,
    vol_regime_sum: f64,
    vol_regime_len: usize,
    last_volume: f64,

    // TSI: double-smoothed momentum
    tsi_ema_long_pc: f64,
    tsi_ema_short_pc: f64,
    tsi_ema_long_apc: f64,
    tsi_ema_short_apc: f64,
    tsi_long_k: f64,
    tsi_short_k: f64,
    tsi: f64,
    tsi_signal: f64,
    tsi_signal_k: f64,
    prev_tsi: f64,
    tsi_bars: usize,

    warmup_bars: usize,
}

impl IncrementalIndicators {
    pub fn atr(&self) -> f64 { self.atr }

    pub fn new(config: &StrategyConfig) -> Self {
        let atr_period = ATR_LENGTH;
        let di_period = DI_LENGTH;
        let warmup_bars = config.min_indicator_bars();

        Self {
            count: 0,
            prev_close: 0.0,
            prev_high: 0.0,
            prev_low: 0.0,
            atr: 0.0,
            atr_period,
            atr_tr_sum: 0.0,
            ema_fast: 0.0,
            ema_slow: 0.0,
            ema_fast_k: 2.0 / (EMA_FAST_LEN_F + 1.0),
            ema_slow_k: 2.0 / (EMA_SLOW_LEN_F + 1.0),
            close_sum_fast: 0.0,
            close_sum_slow: 0.0,
            trend_ema: 0.0,
            trend_ema_k: 2.0 / (IV_LOOKBACK_BARS_F + 1.0),
            trend_ema_len: IV_LOOKBACK_BARS,
            close_sum_trend: 0.0,
            smooth_plus_dm: 0.0,
            smooth_minus_dm: 0.0,
            smooth_tr: 0.0,
            adx: 0.0,
            di_period,
            di_tr_sum: 0.0,
            plus_dm_sum: 0.0,
            minus_dm_sum: 0.0,
            dx_sum: 0.0,
            dx_count: 0,

            atr_regime_ema: 0.0,
            atr_regime_ema_k: 2.0 / (ATR_REGIME_EMA_LEN_F + 1.0),
            atr_regime_bars: 0,
            atr_regime_sum: 0.0,
            atr_regime_len: ATR_REGIME_EMA_LEN,

            vol_regime_ema: 0.0,
            vol_regime_ema_k: 2.0 / (ATR_REGIME_EMA_LEN_F + 1.0),
            vol_regime_bars: 0,
            vol_regime_sum: 0.0,
            vol_regime_len: ATR_REGIME_EMA_LEN,
            last_volume: 0.0,

            tsi_ema_long_pc: 0.0,
            tsi_ema_short_pc: 0.0,
            tsi_ema_long_apc: 0.0,
            tsi_ema_short_apc: 0.0,
            tsi_long_k: 2.0 / (TSI_LONG_LEN_F + 1.0),
            tsi_short_k: 2.0 / (TSI_SHORT_LEN_F + 1.0),
            tsi: 0.0,
            tsi_signal: 0.0,
            tsi_signal_k: 2.0 / (TSI_SIGNAL_LEN_F + 1.0),
            prev_tsi: 0.0,
            tsi_bars: 0,

            warmup_bars,
        }
    }

    pub fn update(&mut self, bar: &OhlcBar) -> Option<IndicatorValues> {
        self.count += 1;
        let n = self.count;

        if n == 1 {
            self.prev_close = bar.close;
            self.prev_high = bar.high;
            self.prev_low = bar.low;
            self.close_sum_fast = bar.close;
            self.close_sum_slow = bar.close;
            return None;
        }

        let price_change = bar.close - self.prev_close;

        // True Range
        let tr = (bar.high - bar.low)
            .max((bar.high - self.prev_close).abs())
            .max((bar.low - self.prev_close).abs());

        // Directional Movement
        let up_move = bar.high - self.prev_high;
        let down_move = self.prev_low - bar.low;
        let plus_dm = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        let minus_dm = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };

        // ATR (Wilder smoothing, seeds with SMA of first atr_period TRs)
        let ap = ATR_LENGTH_F;
        if n <= self.atr_period + 1 {
            self.atr_tr_sum += tr;
            if n == self.atr_period + 1 {
                self.atr = self.atr_tr_sum / ap;
            }
        } else {
            self.atr = (self.atr * (ap - 1.0) + tr) / ap;
        }

        // ATR regime EMA: slow EMA of ATR for vol-regime detection
        if self.atr > 0.0 && self.atr_regime_len > 0 {
            self.atr_regime_bars += 1;
            if self.atr_regime_bars <= self.atr_regime_len {
                self.atr_regime_sum += self.atr;
                if self.atr_regime_bars == self.atr_regime_len {
                    self.atr_regime_ema = self.atr_regime_sum / ATR_REGIME_EMA_LEN_F;
                }
            } else {
                self.atr_regime_ema = self.atr * self.atr_regime_ema_k
                    + self.atr_regime_ema * (1.0 - self.atr_regime_ema_k);
            }
        }

        // Volume regime EMA: slow EMA of volume for volume regime detection
        self.last_volume = bar.volume;
        if bar.volume > 0.0 && self.vol_regime_len > 0 {
            self.vol_regime_bars += 1;
            if self.vol_regime_bars <= self.vol_regime_len {
                self.vol_regime_sum += bar.volume;
                if self.vol_regime_bars == self.vol_regime_len {
                    self.vol_regime_ema = self.vol_regime_sum / ATR_REGIME_EMA_LEN_F;
                }
            } else {
                self.vol_regime_ema = bar.volume * self.vol_regime_ema_k
                    + self.vol_regime_ema * (1.0 - self.vol_regime_ema_k);
            }
        }

        // DI / ADX (Wilder smoothing, separate from ATR)
        let dp = DI_LENGTH_F;
        if n <= self.di_period + 1 {
            self.di_tr_sum += tr;
            self.plus_dm_sum += plus_dm;
            self.minus_dm_sum += minus_dm;
            if n == self.di_period + 1 {
                self.smooth_tr = self.di_tr_sum / dp;
                self.smooth_plus_dm = self.plus_dm_sum / dp;
                self.smooth_minus_dm = self.minus_dm_sum / dp;
            }
        } else {
            self.smooth_tr = (self.smooth_tr * (dp - 1.0) + tr) / dp;
            self.smooth_plus_dm = (self.smooth_plus_dm * (dp - 1.0) + plus_dm) / dp;
            self.smooth_minus_dm = (self.smooth_minus_dm * (dp - 1.0) + minus_dm) / dp;
        }

        let dx = if n > self.di_period + 1 && self.smooth_tr > 0.0 {
            let pdi = (self.smooth_plus_dm / self.smooth_tr) * 100.0;
            let mdi = (self.smooth_minus_dm / self.smooth_tr) * 100.0;
            let di_sum = pdi + mdi;
            if di_sum > 0.0 {
                ((pdi - mdi).abs() / di_sum) * 100.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        if n > self.di_period + 1 {
            self.dx_count += 1;
            if self.dx_count <= self.di_period {
                self.dx_sum += dx;
                if self.dx_count == self.di_period {
                    self.adx = self.dx_sum / dp;
                }
            } else {
                self.adx = (self.adx * (dp - 1.0) + dx) / dp;
            }
        }

        // EMA fast / slow
        if n <= EMA_FAST_LEN {
            self.close_sum_fast += bar.close;
            if n == EMA_FAST_LEN {
                self.ema_fast = self.close_sum_fast / EMA_FAST_LEN_F;
            }
        } else {
            self.ema_fast = bar.close * self.ema_fast_k + self.ema_fast * (1.0 - self.ema_fast_k);
        }

        if n <= EMA_SLOW_LEN {
            self.close_sum_slow += bar.close;
            if n == EMA_SLOW_LEN {
                self.ema_slow = self.close_sum_slow / EMA_SLOW_LEN_F;
            }
        } else {
            self.ema_slow = bar.close * self.ema_slow_k + self.ema_slow * (1.0 - self.ema_slow_k);
        }

        // Trend EMA (~5-day)
        if n <= self.trend_ema_len {
            self.close_sum_trend += bar.close;
            if n == self.trend_ema_len {
                self.trend_ema = self.close_sum_trend / IV_LOOKBACK_BARS_F;
            }
        } else {
            self.trend_ema =
                bar.close * self.trend_ema_k + self.trend_ema * (1.0 - self.trend_ema_k);
        }

        // TSI: double-smoothed momentum
        let abs_pc = price_change.abs();
        self.tsi_bars += 1;
        if self.tsi_bars == 1 {
            self.tsi_ema_long_pc = price_change;
            self.tsi_ema_long_apc = abs_pc;
            self.tsi_ema_short_pc = price_change;
            self.tsi_ema_short_apc = abs_pc;
        } else {
            self.tsi_ema_long_pc = price_change * self.tsi_long_k
                + self.tsi_ema_long_pc * (1.0 - self.tsi_long_k);
            self.tsi_ema_long_apc = abs_pc * self.tsi_long_k
                + self.tsi_ema_long_apc * (1.0 - self.tsi_long_k);
            self.tsi_ema_short_pc = self.tsi_ema_long_pc * self.tsi_short_k
                + self.tsi_ema_short_pc * (1.0 - self.tsi_short_k);
            self.tsi_ema_short_apc = self.tsi_ema_long_apc * self.tsi_short_k
                + self.tsi_ema_short_apc * (1.0 - self.tsi_short_k);
        }
        self.prev_tsi = self.tsi;
        self.tsi = if self.tsi_ema_short_apc.abs() > 1e-15 {
            (self.tsi_ema_short_pc / self.tsi_ema_short_apc) * 100.0
        } else {
            0.0
        };
        if self.tsi_bars == 1 {
            self.tsi_signal = self.tsi;
        } else {
            self.tsi_signal = self.tsi * self.tsi_signal_k
                + self.tsi_signal * (1.0 - self.tsi_signal_k);
        }

        // Store previous bar values
        self.prev_close = bar.close;
        self.prev_high = bar.high;
        self.prev_low = bar.low;

        if n < self.warmup_bars {
            return None;
        }

        let tsi_bullish = self.tsi > self.tsi_signal
            || (self.tsi < self.tsi_signal && self.tsi > self.prev_tsi);

        let atr_regime_ratio = if self.atr_regime_ema > 0.0 && self.atr_regime_bars >= self.atr_regime_len {
            self.atr / self.atr_regime_ema
        } else {
            1.0
        };

        Some(IndicatorValues {
            atr: self.atr,
            ema_fast: self.ema_fast,
            ema_slow: self.ema_slow,
            adx: self.adx,
            trend_ema: self.trend_ema,
            tsi: self.tsi,
            tsi_bullish,
            atr_regime_ratio,
            atr_regime_ema: self.atr_regime_ema,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToF64;
    fn test_config() -> StrategyConfig {
        StrategyConfig::default()
    }

    fn gen_bars(count: usize, base: f64) -> Vec<OhlcBar> {
        let mut bars = Vec::new();
        let mut price = base;
        for i in 0..count {
            let mv = (0.3 * i.to_f64()).sin() * 0.5 + 0.05;
            price += mv;
            bars.push(OhlcBar {
                timestamp: chrono::Utc::now(),
                open: price - 0.2,
                high: price + 0.5,
                low: price - 0.5,
                close: price,
                volume: 10_000.0 + i.to_f64() * 100.0,
            });
        }
        bars
    }

    #[test]
    fn returns_none_before_warmup() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(20, 100.0);
        for bar in &bars {
            assert!(engine.update(bar).is_none());
        }
    }

    #[test]
    fn returns_values_after_warmup() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(80, 100.0);
        let mut got_some = false;
        for bar in &bars {
            if engine.update(bar).is_some() {
                got_some = true;
            }
        }
        assert!(got_some);
    }

    #[test]
    fn two_engines_same_data_same_results() {
        let cfg = test_config();
        let bars = gen_bars(80, 100.0);
        let mut e1 = IncrementalIndicators::new(&cfg);
        let mut e2 = IncrementalIndicators::new(&cfg);
        for bar in &bars {
            let r1 = e1.update(bar);
            let r2 = e2.update(bar);
            match (r1, r2) {
                (Some(a), Some(b)) => {
                    assert!((a.atr - b.atr).abs() < 1e-10);
                    assert!((a.ema_fast - b.ema_fast).abs() < 1e-10);
                    assert!((a.adx - b.adx).abs() < 1e-10);
                }
                (None, None) => {}
                _ => panic!("mismatched warmup"),
            }
        }
    }

    #[test]
    fn atr_always_positive() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(80, 100.0);
        for bar in &bars {
            if let Some(r) = engine.update(bar) {
                assert!(r.atr > 0.0);
            }
        }
    }

    #[test]
    fn ema_tracks_close_prices() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(80, 200.0);
        let mut last = None;
        for bar in &bars {
            if let Some(r) = engine.update(bar) {
                last = Some(r);
            }
        }
        let r = last.unwrap();
        assert!(r.ema_fast > 190.0 && r.ema_fast < 240.0);
        assert!(r.ema_slow > 190.0 && r.ema_slow < 240.0);
        assert!(r.trend_ema > 190.0 && r.trend_ema < 240.0);
    }

    #[test]
    fn adx_in_range() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(80, 100.0);
        for bar in &bars {
            if let Some(r) = engine.update(bar) {
                assert!(r.adx >= 0.0 && r.adx <= 100.0);
            }
        }
    }

    #[test]
    fn results_change_over_time() {
        let cfg = test_config();
        let mut engine = IncrementalIndicators::new(&cfg);
        let bars = gen_bars(80, 100.0);
        let mut results = Vec::new();
        for bar in &bars {
            if let Some(r) = engine.update(bar) {
                results.push(r);
            }
        }
        assert!(results.len() > 2);
        let first = &results[0];
        let last = &results[results.len() - 1];
        let changed =
            first.atr != last.atr || first.ema_fast != last.ema_fast || first.adx != last.adx;
        assert!(changed);
    }

}
