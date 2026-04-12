/// Arguments for [`ZoneState::track_wall`].
#[derive(Debug, Clone, Copy)]
pub struct WallTrackParams {
    pub wall: f64,
    pub width: f64,
    pub reset_pct: f64,
}

impl WallTrackParams {}

/// OHLC + ATR slice for [`ZoneState::tick`].
#[derive(Debug, Clone, Copy)]
pub struct ZoneTickBar {
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub atr: f64,
}

impl ZoneTickBar {}

/// Tracked level + band width for [`ZoneState::reset`].
#[derive(Debug, Clone, Copy)]
pub struct ZoneLevelWidth {
    pub level: f64,
    pub width: f64,
}

impl ZoneLevelWidth {}

/// Tracks price behavior around a horizontal level (put wall).
/// Accumulates a continuous proximity score — smoother than integer touch counts.
#[derive(Debug, Clone)]
pub struct ZoneState {
    pub level: f64,
    pub width: f64,
    pub zone_score: f64,
    pub is_in_zone: bool,
    pub anchor_level: f64,
    prev_anchor: f64,
    prev_score: f64,
    /// Bars since price pierced below the anchor level (low < anchor).
    pub pierce_bars_ago: i32,
    pub max_pierce_depth: f64,
    /// Bars since close was comfortably above the wall (close > wall + 1 ATR).
    /// -1 = never above.
    pub bars_since_above: i32,
}

impl Default for ZoneState {
    fn default() -> Self {
        Self {
            level: 0.0,
            width: 0.0,
            zone_score: 0.0,
            is_in_zone: false,
            anchor_level: 0.0,
            prev_anchor: 0.0,
            prev_score: 0.0,
            pierce_bars_ago: -1,
            max_pierce_depth: 0.0,
            bars_since_above: -1,
        }
    }
}

impl ZoneState {
    pub fn reset(&mut self, z: ZoneLevelWidth) {
        self.level = z.level;
        self.width = z.width;
        self.zone_score = 0.0;
        self.is_in_zone = false;
        self.anchor_level = z.level;
    }

    fn matches_prev(&self, wall: f64) -> bool {
        self.prev_anchor > 0.0
            && ((wall - self.prev_anchor) / self.prev_anchor).abs() < 0.005
    }

    pub fn track_wall(&mut self, p: WallTrackParams) {
        if p.wall <= 0.0 { return; }
        if self.level <= 0.0 {
            self.reset(ZoneLevelWidth { level: p.wall, width: p.width });
            return;
        }
        let shift = ((p.wall - self.level) / self.level).abs();
        if shift > p.reset_pct {
            if self.matches_prev(p.wall) {
                let restored = self.prev_score;
                self.prev_anchor = 0.0;
                self.prev_score = 0.0;
                self.level = p.wall;
                self.width = p.width;
                self.zone_score = restored;
                self.anchor_level = p.wall;
            } else {
                self.prev_anchor = self.anchor_level;
                self.prev_score = self.zone_score;
                self.reset(ZoneLevelWidth { level: p.wall, width: p.width });
            }
        } else {
            self.level = p.wall;
            self.width = p.width;
        }
    }

    /// Feed a new bar. Accumulates proximity score with Gaussian falloff.
    pub fn tick(&mut self, b: &ZoneTickBar) {
        if self.level <= 0.0 || b.atr <= 0.0 { return; }

        let dist_atr = (b.low - self.level).abs() / b.atr;
        self.is_in_zone = dist_atr < 1.0 && b.close >= self.level - b.atr;
        let proximity = (-dist_atr * dist_atr).exp();
        let compression = (1.0 - (b.high - b.low) / b.atr).max(0.0);
        self.zone_score += proximity * (1.0 + compression);

        if b.close > self.level + b.atr {
            self.bars_since_above = 0;
        } else if self.bars_since_above >= 0 {
            self.bars_since_above += 1;
        }

        if self.anchor_level > 0.0 && b.low < self.anchor_level {
            let depth = (self.anchor_level - b.low) / b.atr;
            if self.pierce_bars_ago <= 0 {
                self.max_pierce_depth = depth;
            } else {
                self.max_pierce_depth = self.max_pierce_depth.max(depth);
            }
            self.pierce_bars_ago = 0;
        } else if self.pierce_bars_ago >= 0 {
            self.pierce_bars_ago += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zone_is_empty() {
        let z = ZoneState::default();
        assert_eq!(z.level, 0.0);
        assert_eq!(z.zone_score, 0.0);
        assert!(!z.is_in_zone);
    }

    #[test]
    fn reset_clears_state() {
        let mut z = ZoneState::default();
        z.zone_score = 5.0;
        z.reset(ZoneLevelWidth { level: 100.0, width: 1.0 });
        assert_eq!(z.level, 100.0);
        assert_eq!(z.zone_score, 0.0);
    }

    #[test]
    fn track_wall_initialises_from_zero() {
        let mut z = ZoneState::default();
        z.track_wall(WallTrackParams { wall: 100.0, width: 1.0, reset_pct: 0.03 });
        assert_eq!(z.level, 100.0);
    }

    #[test]
    fn track_wall_resets_on_large_shift() {
        let mut z = ZoneState::default();
        z.track_wall(WallTrackParams { wall: 100.0, width: 1.0, reset_pct: 0.03 });
        z.zone_score = 5.0;
        z.track_wall(WallTrackParams { wall: 120.0, width: 1.0, reset_pct: 0.03 });
        assert_eq!(z.level, 120.0);
        assert_eq!(z.zone_score, 0.0);
    }

    #[test]
    fn tick_accumulates_score() {
        let mut z = ZoneState::default();
        z.reset(ZoneLevelWidth { level: 100.0, width: 2.0 });
        z.tick(&ZoneTickBar { close: 100.5, low: 99.8, high: 100.8, atr: 1.5 });
        assert!(z.zone_score > 0.0);
        assert!(z.is_in_zone);
    }

    #[test]
    fn score_grows_with_repeated_visits() {
        let mut z = ZoneState::default();
        z.reset(ZoneLevelWidth { level: 100.0, width: 2.0 });
        z.tick(&ZoneTickBar { close: 100.5, low: 99.8, high: 100.8, atr: 1.5 });
        let s1 = z.zone_score;
        z.tick(&ZoneTickBar { close: 102.0, low: 101.5, high: 102.5, atr: 1.5 });
        assert!(!z.is_in_zone);
        z.tick(&ZoneTickBar { close: 100.5, low: 99.8, high: 100.8, atr: 1.5 });
        assert!(z.zone_score > s1);
    }
}
