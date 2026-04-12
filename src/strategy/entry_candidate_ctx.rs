use crate::config::{StrategyConfig, Ticker};
use crate::strategy::daily_state::DailyState;
use crate::strategy::indicators::IndicatorValues;
use crate::strategy::signals::SignalState;
use crate::strategy::entries::BarCtx;
use crate::types::{GexProfile, OhlcBar, TradeSignal};

/// Shared refs for [`crate::strategy::engine::StrategyEngine::build_entry_candidate`].
pub struct EntryCandidateBuildCtx<'a> {
    pub signal: &'a TradeSignal,
    pub bar: &'a OhlcBar,
    pub gex: &'a GexProfile,
    pub indicators: &'a IndicatorValues,
    pub config: &'a StrategyConfig,
    pub ticker: Ticker,
}

impl<'a> EntryCandidateBuildCtx<'a> {
    /// Build a [`BarCtx`] from this build context + the engine's signal state.
    #[inline]
    pub fn bar_ctx<'b>(&'b self, state: &'b SignalState) -> BarCtx<'b>
    where
        'a: 'b,
    {
        BarCtx::new(state, self.bar, self.gex, self.indicators, self.config, self.ticker)
    }
}

/// [`EntryCandidateBuildCtx`] plus entry gate fields for [`crate::strategy::engine::StrategyEngine::check_entry_candidate`].
pub struct EntryCandidateCheckCtx<'a> {
    pub build: EntryCandidateBuildCtx<'a>,
    pub position_open: bool,
    pub daily: &'a DailyState,
}

impl<'a> EntryCandidateCheckCtx<'a> {}
