use crate::strategy::signals::SignalState;

/// Smoothed put / call wall levels from [`SignalState`].
#[derive(Debug, Clone, Copy)]
pub struct SmoothedWalls {
    pub pw: f64,
    pub cw: f64,
}

impl SignalState {
    pub(super) fn smoothed_walls(&self) -> SmoothedWalls {
        SmoothedWalls {
            pw: self.smoothed_put_wall(),
            cw: self.smoothed_call_wall(),
        }
    }
}

/// Wall spread in ATR vs configured min/max band (VF / WB guards).
#[derive(Debug, Clone, Copy)]
pub struct SpreadBandInputs<'a> {
    pub spread_atr: f64,
    pub min: f64,
    pub max: f64,
    pub prefix: &'a str,
}

impl<'a> SpreadBandInputs<'a> {
    #[inline]
    pub fn new(spread_atr: f64, min: f64, max: f64, prefix: &'a str) -> Self {
        Self {
            spread_atr,
            min,
            max,
            prefix,
        }
    }

    /// Same acceptance predicate as [`Self::validate`] (VF/WB spread band; `max ≥ 900` disables the ceiling).
    #[inline]
    pub fn spread_in_band(spread_atr: f64, min: f64, max: f64) -> bool {
        spread_atr >= min && (max >= 900.0 || spread_atr <= max)
    }

    pub fn validate(self) -> Result<f64, String> {
        if Self::spread_in_band(self.spread_atr, self.min, self.max) {
            return Ok(self.spread_atr);
        }
        if self.spread_atr < self.min {
            return Err(format!(
                "{prefix}_spread_narrow({spread_atr:.1}<{min:.1})",
                prefix = self.prefix,
                spread_atr = self.spread_atr,
                min = self.min,
            ));
        }
        Err(format!(
            "{prefix}_spread_wide({spread_atr:.1}>{max:.1})",
            prefix = self.prefix,
            spread_atr = self.spread_atr,
            max = self.max,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::signals::SignalState;

    #[test]
    fn smoothed_walls_returns_state() {
        let mut s = SignalState::default();
        s.smoothed_walls.smoothed_pw = 100.0;
        s.smoothed_walls.smoothed_cw = 110.0;
        let SmoothedWalls { pw, cw } = s.smoothed_walls();
        assert!((pw - 100.0).abs() < 0.01);
        assert!((cw - 110.0).abs() < 0.01);
    }

    #[test]
    fn spread_ok() {
        let r = SpreadBandInputs::new(5.0, 1.0, 10.0, "t").validate();
        assert!((r.unwrap() - 5.0).abs() < 0.01);
    }

    #[test]
    fn spread_too_narrow() {
        let r = SpreadBandInputs::new(0.5, 3.0, 10.0, "t").validate();
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("narrow"));
    }

    #[test]
    fn spread_too_wide() {
        let r = SpreadBandInputs::new(15.0, 1.0, 10.0, "t").validate();
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("wide"));
    }

    #[test]
    fn spread_max_disabled_at_900() {
        let r = SpreadBandInputs::new(4999.0, 1.0, 999.0, "t").validate();
        assert!(r.is_ok());
    }

    #[test]
    fn spread_in_band_matches_validate() {
        for &(spread, min, max) in &[(5.0, 1.0, 10.0), (0.5, 3.0, 10.0), (15.0, 1.0, 10.0), (4999.0, 1.0, 999.0)] {
            let ok = SpreadBandInputs::spread_in_band(spread, min, max);
            let v = SpreadBandInputs::new(spread, min, max, "t").validate();
            assert_eq!(ok, v.is_ok(), "spread={spread} min={min} max={max}");
        }
    }
}
