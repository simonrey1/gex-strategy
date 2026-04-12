use crate::strategy::slot_sizing::{
    EntryAtrTsi, EntryOpenLineQuote, EntryPrepareCtx, EntryPrepareInputs, EntryPriceConfig, PrepareEntryError,
    PreparedEntry, SizedEntryBrackets, StopsRegimeCtx,
};
use crate::types::Signal;

/// Snapshot of the signal/indicator/GEX state at entry signal time.
pub struct EntryCandidateData {
    pub signal: Signal,
    pub reason: String,
    pub entry_price: f64,
    pub atr_tsi: EntryAtrTsi,
    pub adx: f64,
    pub net_gex: f64,
    pub gex_spot: f64,
    /// CW distance in ATR at signal time — used for WB spread-based TP. 0 = not applicable.
    pub tp_cap_atr: f64,
}

impl EntryCandidateData {
    #[inline]
    pub fn atr(&self) -> f64 {
        self.atr_tsi.atr
    }

    /// OPEN stdout: signal-row price + sized brackets + entry reason.
    #[inline]
    pub fn open_line_quote(&self, b: &SizedEntryBrackets) -> EntryOpenLineQuote<'_> {
        EntryOpenLineQuote { brackets: *b, entry_price: self.entry_price, reason: self.reason.as_str() }
    }

    #[inline]
    pub fn tsi(&self) -> f64 {
        self.atr_tsi.tsi
    }

    /// Frozen bracket inputs at `atr_regime_ratio` (engine / indicators at signal time).
    #[inline]
    pub fn entry_prepare_inputs(&self, atr_regime_ratio: f64) -> EntryPrepareInputs {
        self.atr_tsi
            .with_atr_regime_ratio(atr_regime_ratio)
            .with_signal_tp(self.signal, self.tp_cap_atr)
    }

    /// Bracket SL/TP with the given ATR regime ratio (e.g. engine snapshot or backtest stored value).
    #[inline]
    pub fn compute_stops(&self, ctx: &StopsRegimeCtx<'_>) -> Option<(f64, f64)> {
        self.entry_prepare_inputs(ctx.atr_regime_ratio)
            .compute_stops_at_price(&EntryPriceConfig::new(self.entry_price, ctx.config))
    }

    /// Frozen [`EntryPrepareInputs`] + sized [`PreparedEntry`] at `trade_price` (signal close, fill, or hypothetical).
    /// Implemented via [`EntryPrepareInputs::bundle_at_price`].
    #[inline]
    pub fn entry_prepare_bundle_at(
        &self,
        trade_price: f64,
        atr_regime_ratio: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<(EntryPrepareInputs, PreparedEntry), PrepareEntryError> {
        self.entry_prepare_inputs(atr_regime_ratio)
            .bundle_at_price(trade_price, ctx)
    }

    /// At [`Self::entry_price`]: same as [`Self::entry_prepare_bundle_at`].
    #[inline]
    pub fn entry_prepare_bundle(
        &self,
        atr_regime_ratio: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<(EntryPrepareInputs, PreparedEntry), PrepareEntryError> {
        self.entry_prepare_bundle_at(self.entry_price, atr_regime_ratio, ctx)
    }

    /// Slot sizing + brackets using signal-bar fields at `trade_price` (live: usually [`Self::entry_price`]).
    #[inline]
    pub fn prepare_entry_at_price(
        &self,
        trade_price: f64,
        atr_regime_ratio: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<PreparedEntry, PrepareEntryError> {
        self.entry_prepare_bundle_at(trade_price, atr_regime_ratio, ctx)
            .map(|(_, prep)| prep)
    }

    /// Same as [`Self::prepare_entry_at_price`] with `trade_price = self.entry_price` (live: signal close).
    #[inline]
    pub fn prepare_entry(
        &self,
        atr_regime_ratio: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<PreparedEntry, PrepareEntryError> {
        self.entry_prepare_bundle(atr_regime_ratio, ctx)
            .map(|(_, prep)| prep)
    }
}
