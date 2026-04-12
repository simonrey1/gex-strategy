use crate::config::{StrategyConfig, Ticker};
use crate::strategy::hurst::HurstTracker;
use crate::strategy::indicators::IndicatorValues;
use crate::strategy::trail_fields::TrailFields;
use crate::strategy::wall_smoother::WallSmoother;
use crate::strategy::wall_trail::TrailCheckInputs;
use crate::types::{GexProfile, OhlcBar};

/// Immutable bar + GEX slice for [`crate::strategy::engine::StrategyEngine::process_gex_bar_pipeline`].
/// `StrategyConfig` and [`crate::strategy::daily_state::DailyState`] are passed separately so
/// [`crate::backtest::types::TickerState::run_gex_bar_pipeline`] /
/// [`crate::live::ticker_state::LiveTickerState::run_gex_bar_pipeline`] can take `&mut self` without
/// `pipe` borrowing from the ticker.
pub struct GexPipelineBar<'a> {
    pub bar: &'a OhlcBar,
    pub gex: &'a GexProfile,
    pub indicators: &'a IndicatorValues,
    pub ticker: Ticker,
    pub verbose: bool,
    /// Runner pre-guard: live = data freshness; backtest = no pending deferred entry.
    pub entry_when: bool,
}

impl<'a> GexPipelineBar<'a> {}


/// Arguments for [`crate::strategy::engine::StrategyEngine::process_gex_bar`].
pub struct ProcessGexBarCtx<'a> {
    pub bar: &'a OhlcBar,
    pub gex: &'a GexProfile,
    pub indicators: &'a IndicatorValues,
    pub smoother: &'a mut WallSmoother,
    pub hurst: &'a mut HurstTracker,
    pub position: Option<TrailFields<'a>>,
    pub config: &'a StrategyConfig,
    pub ticker: Ticker,
    pub verbose: bool,
}

impl<'a> ProcessGexBarCtx<'a> {
    /// Struct literal shorthand sourcing bar/GEX/indicators/ticker/verbose from [`GexPipelineBar`].
    #[inline]
    pub fn from_pipeline_bar(
        pipe: &GexPipelineBar<'a>,
        smoother: &'a mut WallSmoother,
        hurst: &'a mut HurstTracker,
        position: Option<TrailFields<'a>>,
        config: &'a StrategyConfig,
    ) -> Self {
        Self {
            bar: pipe.bar,
            gex: pipe.gex,
            indicators: pipe.indicators,
            smoother,
            hurst,
            position,
            config,
            ticker: pipe.ticker,
            verbose: pipe.verbose,
        }
    }

    /// Wall-trail scalars after [`crate::strategy::engine::StrategyEngine::apply_gex_walls`]:
    /// pass [`crate::strategy::signals::SignalState::smoothed_put_wall`] and pre-computed `gex_norm`.
    #[inline]
    pub fn trail_check_inputs(&self, smoothed_pw: f64, gex_norm: f64) -> TrailCheckInputs {
        TrailCheckInputs::new(
            smoothed_pw,
            self.indicators.bar_vol_regime(self.bar.close),
            gex_norm,
        )
    }
}
