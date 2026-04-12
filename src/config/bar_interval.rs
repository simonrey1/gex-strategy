//! Strategy bar cadence: bar length in whole minutes.
//!
//! Use [`Minutes`] for every value that means “bar interval in minutes”. Convert to
//! `i64` (aligned timestamps), `u64` (durations), or `usize` (strides vs 1‑min bars)
//! only through the helpers in this module.

#[inline]
fn u32_to_usize(x: u32) -> usize {
    usize::try_from(x).unwrap_or(0)
}

/// Bar interval in whole minutes (e.g. 15 → 15‑minute bars).
pub type Minutes = u32;

/// Default 15‑minute bars (strategy execution, GEX cache, live poll cadence).
pub const BAR_INTERVAL_MINUTES: Minutes = 15;

/// Seconds per bar bucket (for aligning Unix timestamps to bar starts).
#[must_use]
#[inline]
pub fn bucket_secs_i64(minutes: Minutes) -> i64 {
    i64::from(minutes) * 60
}

/// Live main loop: sleep between bar polls (`tokio::time::Duration::from_millis`).
#[must_use]
#[inline]
pub fn poll_interval_ms(minutes: Minutes) -> u64 {
    u64::from(minutes) * 60_000
}

/// Background GEX poll: sleep in one‑second ticks (`Duration::from_secs(1)` in a loop).
#[must_use]
#[inline]
pub fn poll_interval_secs_u64(minutes: Minutes) -> u64 {
    u64::from(minutes) * 60
}

/// How many 1‑minute slots fit in a trading day for this interval (e.g. 390 / 15).
#[must_use]
#[inline]
pub fn bars_per_day_1m_div_interval(bars_per_day_1m: usize, minutes: Minutes) -> usize {
    let stride = u32_to_usize(minutes.max(1)).max(1);
    bars_per_day_1m / stride
}

/// Stride in 1‑minute bars (capacity / indexing vs `bars.len()`).
#[must_use]
#[inline]
pub fn minutes_stride_usize(minutes: Minutes) -> usize {
    u32_to_usize(minutes)
}

/// Floor‑divide 1‑minute bar count into strategy bars (e.g. 45 → 3 when `minutes == 15`).
#[must_use]
#[inline]
pub fn strategy_bars_from_1m_count(bars_1m: i64, minutes: Minutes) -> i64 {
    let m = i64::from(minutes.max(1));
    bars_1m / m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_bars_from_1m_count_divides_by_interval() {
        assert_eq!(strategy_bars_from_1m_count(45, 15), 3);
        assert_eq!(strategy_bars_from_1m_count(44, 15), 2);
        assert_eq!(strategy_bars_from_1m_count(0, BAR_INTERVAL_MINUTES), 0);
    }

    #[test]
    fn bars_per_day_1m_div_interval_matches_stride() {
        let per_day = bars_per_day_1m_div_interval(390, BAR_INTERVAL_MINUTES);
        assert_eq!(per_day, 390 / u32_to_usize(BAR_INTERVAL_MINUTES).max(1));
    }
}
