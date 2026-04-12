use std::fmt;
use std::ops::Deref;

use crate::config::{StrategyConfig, Ticker};
use crate::strategy::config_stops::StopBracketInputs;
use crate::strategy::position_cash::{EntrySharesInputs, PositionCash};
pub use crate::types::{AtrRegimeTsi, EntryAtrTsi};
use crate::types::Signal;

/// Backwards-compatible name for [`AtrRegimeTsi`] (prepare / deferred fills).
pub type EntryRegimeFields = AtrRegimeTsi;

impl AtrRegimeTsi {
    #[inline]
    pub const fn with_signal_tp(self, signal: Signal, tp_cap_atr: f64) -> EntryPrepareInputs {
        EntryPrepareInputs {
            regime: self,
            signal,
            tp_cap_atr,
        }
    }
}

/// Regime + signal + WB TP cap — full input for [`SlotSizing::prepare_entry`].
#[derive(Debug, Clone, Copy)]
pub struct EntryPrepareInputs {
    pub regime: EntryRegimeFields,
    pub signal: Signal,
    pub tp_cap_atr: f64,
}

/// Trade price + strategy config — one fill price for [`EntryPrepareInputs::compute_stops_at_price`]
/// and [`SlotSizing::try_size_shares`].
#[derive(Debug, Clone, Copy)]
pub struct EntryPriceConfig<'a> {
    pub price: f64,
    pub config: &'a StrategyConfig,
}

impl<'a> EntryPriceConfig<'a> {
    #[inline]
    pub const fn new(price: f64, config: &'a StrategyConfig) -> Self {
        Self { price, config }
    }
}

/// ATR regime ratio + config — bracket SL/TP from a signal snapshot without slot sizing.
#[derive(Debug, Clone, Copy)]
pub struct StopsRegimeCtx<'a> {
    pub atr_regime_ratio: f64,
    pub config: &'a StrategyConfig,
}

impl<'a> StopsRegimeCtx<'a> {
    #[inline]
    pub const fn new(atr_regime_ratio: f64, config: &'a StrategyConfig) -> Self {
        Self { atr_regime_ratio, config }
    }
}

impl EntryPrepareInputs {
    #[inline]
    pub fn to_atr_tsi(&self) -> EntryAtrTsi {
        self.regime.to_atr_tsi()
    }

    /// SL/TP at `pc.price` using the same fields as [`SlotSizing::prepare_entry`].
    #[inline]
    pub fn compute_stops_at_price(&self, pc: &EntryPriceConfig<'_>) -> Option<(f64, f64)> {
        pc.config.compute_stops_for(&StopBracketInputs {
            entry_price: pc.price,
            regime: self.regime,
            signal: self.signal,
            tp_cap_atr: self.tp_cap_atr,
        })
    }

}

/// Slot-level context for entry sizing, shared by both runners.
pub struct SlotSizing {
    pub open_positions: usize,
    pub max_positions: usize,
    pub equity: f64,
    pub per_position_div: f64,
}

impl SlotSizing {
    #[must_use]
    #[inline]
    pub const fn new(
        open_positions: usize,
        max_positions: usize,
        equity: f64,
        per_position_div: f64,
    ) -> Self {
        Self {
            open_positions,
            max_positions,
            equity,
            per_position_div,
        }
    }

    /// Size in shares if under the slot cap and `max_position_pct` allows a non-zero allocation.
    #[inline]
    pub fn try_size_shares(&self, pc: &EntryPriceConfig<'_>) -> Option<u32> {
        if self.open_positions >= self.max_positions {
            return None;
        }
        let effective_pos_pct = pc.config.max_position_pct / self.per_position_div;
        let shares = PositionCash::entry_shares(&EntrySharesInputs::new(
            self.equity,
            effective_pos_pct,
            pc.price,
        ));
        if shares == 0 { None } else { Some(shares) }
    }

    /// Size the slot and compute SL/TP at a concrete trade price.
    #[inline]
    pub fn prepare_entry(
        &self,
        trade_price: f64,
        config: &StrategyConfig,
        inputs: EntryPrepareInputs,
    ) -> Result<PreparedEntry, PrepareEntryError> {
        inputs.sized_at_price(trade_price, &EntryPrepareCtx::new(self, config))
    }
}

/// Slot + [`StrategyConfig`] for prepare/sized calls (live, backtest, tests).
#[derive(Clone, Copy)]
pub struct EntryPrepareCtx<'a, 'b> {
    pub slot: &'a SlotSizing,
    pub config: &'b StrategyConfig,
}

impl<'a, 'b> EntryPrepareCtx<'a, 'b> {
    #[inline]
    pub const fn new(slot: &'a SlotSizing, config: &'b StrategyConfig) -> Self {
        Self { slot, config }
    }
}

impl EntryPrepareInputs {
    /// Shares + SL/TP at `trade_price` (signal close, deferred fill, or hypothetical).
    /// [`SlotSizing::prepare_entry`] delegates here — single implementation.
    #[inline]
    pub fn sized_at_price(
        self,
        trade_price: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<PreparedEntry, PrepareEntryError> {
        let pc = EntryPriceConfig::new(trade_price, ctx.config);
        let Some(shares) = ctx.slot.try_size_shares(&pc) else {
            return Err(PrepareEntryError::SlotFull);
        };
        let Some((stop_loss, take_profit)) = self.compute_stops_at_price(&pc) else {
            return Err(PrepareEntryError::StopsFailed);
        };
        Ok(PreparedEntry {
            brackets: SizedEntryBrackets { shares, stop_loss: stop_loss, take_profit: take_profit },
        })
    }

    /// Same tuple shape both runners use: frozen inputs + sized brackets at `trade_price`.
    #[inline]
    pub fn bundle_at_price(
        self,
        trade_price: f64,
        ctx: &EntryPrepareCtx<'_, '_>,
    ) -> Result<(EntryPrepareInputs, PreparedEntry), PrepareEntryError> {
        let prep = self.sized_at_price(trade_price, ctx)?;
        Ok((self, prep))
    }
}

/// Sized SL/TP + share count for [`EntryCandidateData::open_line_quote`].
#[derive(Debug, Clone, Copy)]
pub struct SizedEntryBrackets {
    pub shares: u32,
    pub stop_loss: f64,
    pub take_profit: f64,
}

impl SizedEntryBrackets {}

/// Exit reason + fill + PnL fields for [`RunnerMode::format_close_line`] / [`RunnerTickerLog::format_close_line`].
#[derive(Debug, Clone, Copy)]
pub struct CloseLineQuote<'a> {
    pub reason: &'a str,
    pub exit_price: f64,
    pub net_pnl: f64,
    pub return_pct: f64,
    pub max_runup_atr: Option<f64>,
}

impl<'a> CloseLineQuote<'a> {
    #[inline]
    pub const fn new(
        reason: &'a str,
        exit_price: f64,
        net_pnl: f64,
        return_pct: f64,
        max_runup_atr: Option<f64>,
    ) -> Self {
        Self {
            reason,
            exit_price,
            net_pnl,
            return_pct,
            max_runup_atr,
        }
    }
}

/// Bracket size + fill price + reason for one OPEN stdout line ([`EntryOpenDailyLog`], [`RunnerTickerLog`]).
#[derive(Debug, Clone, Copy)]
pub struct EntryOpenLineQuote<'a> {
    pub brackets: SizedEntryBrackets,
    pub entry_price: f64,
    pub reason: &'a str,
}

impl<'a> EntryOpenLineQuote<'a> {}

impl<'a> Deref for EntryOpenLineQuote<'a> {
    type Target = SizedEntryBrackets;
    #[inline]
    fn deref(&self) -> &SizedEntryBrackets {
        &self.brackets
    }
}

/// Shares + bracket SL/TP after slot sizing and stop math.
#[derive(Debug, Clone, Copy)]
pub struct PreparedEntry {
    pub brackets: SizedEntryBrackets,
}

impl Deref for PreparedEntry {
    type Target = SizedEntryBrackets;
    #[inline]
    fn deref(&self) -> &SizedEntryBrackets {
        &self.brackets
    }
}

/// Fill price + reason for [`PreparedEntry::open_line_quote`] (deferred fill vs signal-row price).
#[derive(Debug, Clone, Copy)]
pub struct PreparedOpenLineCtx<'a> {
    pub entry_price: f64,
    pub reason: &'a str,
}

impl<'a> PreparedOpenLineCtx<'a> {
    #[inline]
    pub const fn new(entry_price: f64, reason: &'a str) -> Self {
        Self { entry_price, reason }
    }
}

impl PreparedEntry {
    /// Fields for one OPEN stdout line (fill price + strategy reason string).
    #[inline]
    pub fn open_line_quote<'a>(&self, ctx: &PreparedOpenLineCtx<'a>) -> EntryOpenLineQuote<'a> {
        EntryOpenLineQuote { brackets: self.brackets, entry_price: ctx.entry_price, reason: ctx.reason }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareEntryError {
    SlotFull,
    StopsFailed,
}

/// Deployment path for log line prefixes — still emits `[live-…]` / `[backtest-…]` in output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerMode {
    /// Broker / real-time path (`[live-{ticker}]`).
    External,
    /// Simulation / deferred-fill path (`[backtest-{ticker}]`).
    Simulated,
}

impl RunnerMode {
    #[inline]
    pub const fn log_prefix(self) -> &'static str {
        match self {
            RunnerMode::External => "live",
            RunnerMode::Simulated => "backtest",
        }
    }

    /// Fixed `(mode, ticker)` for log helpers — avoids passing [`RunnerMode`] + ticker through every call site.
    #[inline]
    pub const fn with_ticker(self, ticker: Ticker) -> RunnerTickerLog {
        RunnerTickerLog { mode: self, ticker }
    }

    /// `[live-AAPL]`-style prefix for log lines.
    #[must_use]
    #[inline]
    pub fn tag(self, ticker: impl fmt::Display) -> String {
        format!("[{}-{}]", self.log_prefix(), ticker)
    }

    /// One log line when SL/TP computation fails after sizing (stderr vs stdout is up to the caller).
    #[inline]
    pub fn reject_stops_failed_line(self, ticker: impl fmt::Display, signal: impl fmt::Display) -> String {
        format!("{} REJECT {signal} | SL computation failed", self.tag(ticker))
    }

    /// Exit line: pass [`None`] for `max_runup_atr` when runup diagnostics are not logged.
    #[must_use]
    pub fn format_close_line(self, ticker: impl fmt::Display, line: &CloseLineQuote<'_>) -> String {
        let pct = crate::types::fmt_pct(line.return_pct);
        let tag = self.tag(ticker);
        match line.max_runup_atr {
            Some(r) => format!(
                "{tag} CLOSE {} @ ${:.2} | pnl=${:.2} ({pct}) runup={r:.1}atr",
                line.reason, line.exit_price, line.net_pnl,
            ),
            None => format!(
                "{tag} CLOSE {} @ ${:.2} | pnl=${:.2} ({pct})",
                line.reason, line.exit_price, line.net_pnl,
            ),
        }
    }
}

/// [`RunnerMode`] + [`Ticker`] bound together for logging (live and backtest each use one fixed mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerTickerLog {
    pub mode: RunnerMode,
    pub ticker: Ticker,
}

impl RunnerTickerLog {
    #[must_use]
    #[inline]
    pub fn tag(self) -> String {
        self.mode.tag(self.ticker)
    }

    #[inline]
    pub fn reject_stops_failed_line(self, signal: impl fmt::Display) -> String {
        self.mode.reject_stops_failed_line(self.ticker, signal)
    }

    #[must_use]
    #[inline]
    pub fn format_close_line(self, line: &CloseLineQuote<'_>) -> String {
        self.mode.format_close_line(self.ticker, line)
    }

    #[must_use]
    #[inline]
    pub fn format_open_line_quote<'a>(
        self,
        kind: EntryOpenLogKind,
        signal: impl fmt::Display,
        entries: u32,
        max_entries: u32,
        q: &EntryOpenLineQuote<'a>,
    ) -> String {
        kind.format_open_line(self.mode, self.ticker, signal, q, entries, max_entries)
    }

    #[must_use]
    #[inline]
    pub fn format_open_line(
        self,
        kind: EntryOpenLogKind,
        signal: impl fmt::Display,
        shares: u32,
        entry_price: f64,
        sl: f64,
        tp: f64,
        reason: &str,
        entries: u32,
        max_entries: u32,
    ) -> String {
        self.format_open_line_quote(
            kind,
            signal,
            entries,
            max_entries,
            &EntryOpenLineQuote { brackets: SizedEntryBrackets { shares, stop_loss: sl, take_profit: tp }, entry_price, reason },
        )
    }
}

/// Commit-at-signal (equity) vs deferred fill (regime ratio) for [`EntryOpenLogKind::format_open_line`].
#[derive(Debug, Clone, Copy)]
pub enum EntryOpenLogKind {
    /// After commit at signal price: `** SIGNAL **` + account equity (typical IBKR path).
    SignalCommit { equity: f64 },
    /// Deferred fill at trade price: `OPEN` + `rr=` (simulated execution delay).
    DeferredFill { atr_regime_ratio: f64 },
}

impl EntryOpenLogKind {
    /// One stdout entry line for this kind and [`RunnerMode`].
    #[must_use]
    pub fn format_open_line(
        self,
        mode: RunnerMode,
        ticker: impl fmt::Display,
        signal: impl fmt::Display,
        q: &EntryOpenLineQuote<'_>,
        entries: u32,
        max_entries: u32,
    ) -> String {
        let tag = mode.tag(ticker);
        match self {
            EntryOpenLogKind::SignalCommit { equity } => format!(
                "{tag} ** SIGNAL ** {signal} {}sh @ ${:.2} | equity=${equity:.0} | SL=${:.2} TP=${:.2} | {} [entry {entries}/{max_entries}]",
                q.shares, q.entry_price, q.stop_loss, q.take_profit, q.reason,
            ),
            EntryOpenLogKind::DeferredFill { atr_regime_ratio } => format!(
                "{tag} OPEN {signal} {}sh @ ${:.2} | SL=${:.2} TP=${:.2} rr={atr_regime_ratio:.2} | {} [entry {entries}/{max_entries}]",
                q.shares, q.entry_price, q.stop_loss, q.take_profit, q.reason,
            ),
        }
    }
}

/// [`RunnerTickerLog`] + daily entry cap — shared by live and backtest OPEN lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryOpenDailyLog {
    rtl: RunnerTickerLog,
    max_entries_per_day: u32,
}

impl EntryOpenDailyLog {
    #[must_use]
    #[inline]
    pub fn format_line_quote<'a>(
        self,
        kind: EntryOpenLogKind,
        entries_today: u32,
        signal: impl fmt::Display,
        q: &EntryOpenLineQuote<'a>,
    ) -> String {
        self.rtl.format_open_line_quote(
            kind,
            signal,
            entries_today,
            self.max_entries_per_day,
            q,
        )
    }

    #[must_use]
    #[inline]
    pub fn format_line(
        self,
        kind: EntryOpenLogKind,
        entries_today: u32,
        signal: impl fmt::Display,
        shares: u32,
        entry_price: f64,
        sl: f64,
        tp: f64,
        reason: &str,
    ) -> String {
        self.format_line_quote(
            kind,
            entries_today,
            signal,
            &EntryOpenLineQuote { brackets: SizedEntryBrackets { shares, stop_loss: sl, take_profit: tp }, entry_price, reason },
        )
    }
}

impl RunnerTickerLog {
    /// Bind [`StrategyConfig::max_entries_per_day`] once; use [`EntryOpenDailyLog::format_line`] for OPEN stdout.
    #[inline]
    pub const fn with_daily_entry_cap(self, max_entries_per_day: u32) -> EntryOpenDailyLog {
        EntryOpenDailyLog {
            rtl: self,
            max_entries_per_day,
        }
    }
}

/// Extension for [`Result`] with [`PrepareEntryError`] — used by [`SlotSizing::try_prepare_bundle`] and tests.
pub trait PrepareOutcomeExt<T> {
    /// [`Ok`] → [`Some`]; slot full → [`None`]; stops failed → `on_stops_failed` then [`None`].
    fn or_prepare_none(self, on_stops_failed: impl FnOnce()) -> Option<T>;
}

impl<T> PrepareOutcomeExt<T> for Result<T, PrepareEntryError> {
    #[inline]
    fn or_prepare_none(self, on_stops_failed: impl FnOnce()) -> Option<T> {
        match self {
            Ok(x) => Some(x),
            Err(PrepareEntryError::SlotFull) => None,
            Err(PrepareEntryError::StopsFailed) => {
                on_stops_failed();
                None
            }
        }
    }
}

impl SlotSizing {
    /// Run `f` with `self`; maps prepare errors through `on_stops_failed` on stops failure.
    #[inline]
    pub fn try_prepare_bundle<T>(
        &self,
        f: impl FnOnce(&SlotSizing) -> Result<T, PrepareEntryError>,
        on_stops_failed: impl FnOnce(),
    ) -> Option<T> {
        PrepareOutcomeExt::or_prepare_none(f(self), on_stops_failed)
    }

    /// Remaining concurrent entry slots (`slots_used` may include pending entries in simulation).
    #[inline]
    pub fn remaining_slots(max_positions: usize, slots_used: usize) -> usize {
        max_positions.saturating_sub(slots_used)
    }
}

/// Portfolio-level sizing inputs for live entry (per call: `slots_used` is current open + pending slots).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortfolioSlotSizing {
    pub slots_used: usize,
    pub max_positions: usize,
    pub per_position_pct_div: f64,
}

impl PortfolioSlotSizing {
    #[inline]
    pub fn slot_sizing(self, equity: f64) -> SlotSizing {
        SlotSizing::new(
            self.slots_used,
            self.max_positions,
            equity,
            self.per_position_pct_div,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{StrategyConfig, Ticker};
    use crate::types::Signal;

    fn slot(open: usize, max: usize, equity: f64, div: f64) -> SlotSizing {
        SlotSizing::new(open, max, equity, div)
    }

    #[test]
    fn try_size_shares_rejects_full_slots() {
        let cfg = StrategyConfig::default();
        assert!(slot(3, 3, 10_000.0, 3.0)
            .try_size_shares(&EntryPriceConfig::new(100.0, &cfg))
            .is_none());
    }

    #[test]
    fn try_size_shares_basic() {
        let cfg = StrategyConfig::default();
        let shares = slot(0, 3, 10_000.0, 3.0).try_size_shares(&EntryPriceConfig::new(100.0, &cfg));
        assert!(shares.is_some());
        let s = shares.unwrap();
        let expected_capital = 10_000.0 * cfg.max_position_pct / 3.0;
        let expected = (expected_capital / 100.0).floor() as u32;
        assert_eq!(s, expected);
    }

    #[test]
    fn prepare_entry_uses_inputs_bundle() {
        let cfg = StrategyConfig::default();
        let s = slot(0, 3, 10_000.0, 3.0);
        let inputs = EntryRegimeFields {
            atr: 2.0,
            atr_regime_ratio: 1.0,
            tsi: 0.0,
        }
        .with_signal_tp(Signal::LongVannaFlip, 0.0);
        let ctx = EntryPrepareCtx::new(&s, &cfg);
        let prep = inputs.sized_at_price(100.0, &ctx).expect("sized_at_price");
        assert!(prep.stop_loss < 100.0 && prep.take_profit > 100.0);
    }

    #[test]
    fn bundle_at_price_matches_sized_and_returns_inputs() {
        let cfg = StrategyConfig::default();
        let s = slot(0, 3, 10_000.0, 3.0);
        let ctx = EntryPrepareCtx::new(&s, &cfg);
        let inputs = EntryRegimeFields {
            atr: 2.0,
            atr_regime_ratio: 1.0,
            tsi: 0.0,
        }
        .with_signal_tp(Signal::LongVannaFlip, 0.0);
        let prep = inputs.sized_at_price(100.0, &ctx).expect("sized_at_price");
        let (same, prep2) = inputs.bundle_at_price(100.0, &ctx).expect("bundle_at_price");
        assert_eq!(inputs.signal, same.signal);
        assert_eq!(inputs.regime.atr, same.regime.atr);
        assert_eq!(prep.shares, prep2.shares);
        assert_eq!(prep.stop_loss, prep2.stop_loss);
        assert_eq!(prep.take_profit, prep2.take_profit);
    }

    #[test]
    fn try_prepare_bundle_matches_or_prepare_none() {
        let cfg = StrategyConfig::default();
        let s = slot(0, 3, 10_000.0, 3.0);
        let inputs = EntryRegimeFields {
            atr: 2.0,
            atr_regime_ratio: 1.0,
            tsi: 0.0,
        }
        .with_signal_tp(Signal::LongVannaFlip, 0.0);
        let ctx = EntryPrepareCtx::new(&s, &cfg);
        let (i1, p1) = inputs
            .bundle_at_price(100.0, &ctx)
            .or_prepare_none(|| panic!("stops"))
            .expect("direct");
        let (i2, p2) = SlotSizing::new(0, 3, 10_000.0, 3.0)
            .try_prepare_bundle(
                |slot| inputs.bundle_at_price(100.0, &EntryPrepareCtx::new(slot, &cfg)),
                || panic!("stops"),
            )
            .expect("via");
        assert_eq!(i1.signal, i2.signal);
        assert_eq!(p1.shares, p2.shares);
        assert_eq!(p1.stop_loss, p2.stop_loss);
        assert_eq!(p1.take_profit, p2.take_profit);
    }

    #[test]
    fn or_prepare_none_ok_and_errors() {
        use std::cell::Cell;
        let called = Cell::new(false);
        assert_eq!(Ok(7u8).or_prepare_none(|| called.set(true)), Some(7));
        assert!(!called.get());

        assert!(Err::<u8, _>(PrepareEntryError::SlotFull)
            .or_prepare_none(|| called.set(true))
            .is_none());
        assert!(!called.get());

        assert!(Err::<u8, _>(PrepareEntryError::StopsFailed)
            .or_prepare_none(|| called.set(true))
            .is_none());
        assert!(called.get());
    }

    #[test]
    fn runner_mode_tag_format() {
        assert_eq!(RunnerMode::External.tag("X"), "[live-X]");
        assert_eq!(RunnerMode::Simulated.tag("Y"), "[backtest-Y]");
    }

    #[test]
    fn reject_stops_failed_line_format() {
        let s = RunnerMode::External.reject_stops_failed_line("AAPL", "VF");
        assert!(
            s.starts_with(&RunnerMode::External.tag("AAPL")) && s.contains("SL computation failed")
        );
    }

    #[test]
    fn format_close_line_runup_optional() {
        let no_runup = RunnerMode::External.format_close_line(
            "T",
            &CloseLineQuote::new("sl", 100.0, -5.0, -0.02, None),
        );
        assert!(!no_runup.contains("runup="));
        let with_runup = RunnerMode::Simulated.format_close_line(
            "T",
            &CloseLineQuote::new("tp", 110.0, 50.0, 0.05, Some(2.5)),
        );
        assert!(with_runup.contains("runup=2.5atr"));
    }

    #[test]
    fn format_entry_open_kinds() {
        let q_commit = EntryOpenLineQuote { brackets: SizedEntryBrackets { shares: 10, stop_loss: 95.0, take_profit: 110.0 }, entry_price: 100.0, reason: "r" };
        let commit = EntryOpenLogKind::SignalCommit { equity: 50_000.0 }.format_open_line(
            RunnerMode::External,
            "X",
            Signal::LongVannaFlip,
            &q_commit,
            1,
            10,
        );
        assert!(commit.contains("SIGNAL") && commit.contains("equity=") && commit.contains("[entry 1/10]"));
        let q_def = EntryOpenLineQuote { brackets: SizedEntryBrackets { shares: 10, stop_loss: 94.0, take_profit: 111.0 }, entry_price: 99.5, reason: "diag" };
        let deferred = EntryOpenLogKind::DeferredFill { atr_regime_ratio: 1.25 }.format_open_line(
            RunnerMode::Simulated,
            "X",
            Signal::LongWallBounce,
            &q_def,
            2,
            10,
        );
        assert!(deferred.contains("OPEN") && deferred.contains("rr=1.25") && deferred.contains("[entry 2/10]"));
    }

    #[test]
    fn portfolio_slot_sizing_matches_slot_sizing_new() {
        let p = PortfolioSlotSizing {
            slots_used: 1,
            max_positions: 4,
            per_position_pct_div: 3.0,
        };
        let eq = 12_000.0;
        let a = p.slot_sizing(eq);
        let b = SlotSizing::new(1, 4, eq, 3.0);
        assert_eq!(a.open_positions, b.open_positions);
        assert_eq!(a.max_positions, b.max_positions);
        assert_eq!(a.equity, b.equity);
        assert_eq!(a.per_position_div, b.per_position_div);
    }

    #[test]
    fn entry_open_daily_log_matches_rtl_format_open_line() {
        let rtl = RunnerMode::Simulated.with_ticker(Ticker::JPM);
        let daily = rtl.with_daily_entry_cap(7);
        let kind = EntryOpenLogKind::DeferredFill { atr_regime_ratio: 1.1 };
        let q = EntryOpenLineQuote { brackets: SizedEntryBrackets { shares: 5, stop_loss: 90.0, take_profit: 110.0 }, entry_price: 100.0, reason: "x" };
        assert_eq!(
            daily.format_line_quote(kind, 2, Signal::LongVannaFlip, &q),
            rtl.format_open_line_quote(kind, Signal::LongVannaFlip, 2, 7, &q),
        );
    }

    #[test]
    fn runner_ticker_log_matches_runner_mode() {
        let rtl = RunnerMode::External.with_ticker(Ticker::AAPL);
        assert_eq!(rtl.tag(), RunnerMode::External.tag(Ticker::AAPL));
        assert_eq!(
            rtl.reject_stops_failed_line(Signal::LongVannaFlip),
            RunnerMode::External.reject_stops_failed_line(Ticker::AAPL, Signal::LongVannaFlip),
        );
        let cl = CloseLineQuote::new("sl", 100.0, -1.0, -0.01, None);
        assert_eq!(
            rtl.format_close_line(&cl),
            RunnerMode::External.format_close_line(Ticker::AAPL, &cl),
        );
    }
}
