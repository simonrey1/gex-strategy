//! Bar-stream indices: monotonic bar counter, spike bar references, spike-window limits.
//! (Distinct from [`super::bar_interval::Minutes`] — clock cadence vs bar-axis position.)

/// Position along the strategy bar stream (`SignalState::bar_index`, `iv_spike_bar`, IV scan).
pub type BarIndex = i64;

/// [`super::strategy::IV_LOOKBACK_BARS`] as [`BarIndex`] for spike-age / compression checks.
pub const IV_LOOKBACK_BARS_INDEX: BarIndex = super::strategy::IV_LOOKBACK_BARS as BarIndex;
