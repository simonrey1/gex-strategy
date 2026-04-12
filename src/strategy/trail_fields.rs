/// Mutable position fields needed for wall trailing / hurst trailing.
pub struct TrailFields<'a> {
    pub stop_loss: &'a mut f64,
    pub highest_put_wall: &'a mut f64,
    pub highest_close: &'a mut f64,
    pub hurst_exhaust_bars: &'a mut u32,
    pub entry_price: f64,
    pub tp: f64,
    pub signal: crate::types::Signal,
}

/// Any position type that carries the fields needed for wall trailing.
pub trait HasTrailFields {
    fn trail_fields(&mut self) -> TrailFields<'_>;
}

/// Maps an open position into [`TrailFields`] for [`crate::strategy::engine::StrategyEngine::process_gex_bar`].
/// Live and backtest both call this before `process_gex_bar`.
#[inline]
pub fn trail_fields_for_position<T: HasTrailFields>(position: Option<&mut T>) -> Option<TrailFields<'_>> {
    position.map(|p| p.trail_fields())
}
