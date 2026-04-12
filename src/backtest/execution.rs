use std::collections::HashMap;

use crate::config::{BacktestConfig, StrategyConfig, Ticker};
use crate::data::hist::ts_key;
use crate::strategy::shared::{
    CloseLineQuote, EntryOpenDailyLog, EntryOpenLogKind, EntryPrepareCtx, PortfolioSlotSizing,
    PreparedOpenLineCtx, RunnerMode, RunnerTickerLog, SlotSizing,
};
use crate::types::{GexProfile, OhlcBar, ToF64};

use super::positions::{Position, PositionCloseCtx, Trade};
use super::types::{ChartExitRecord, TickerState};

pub struct ExecContext<'a> {
    pub ticker: Ticker,
    pub bc: &'a BacktestConfig,
    pub config: &'a StrategyConfig,
    pub portfolio_equity: f64,
    pub max_pos: usize,
    pub per_position_pct_div: f64,
    pub starting_capital: f64,
    pub verbosity: u8,
}

impl ExecContext<'_> {
    /// Backtest execution always uses [`RunnerMode::Simulated`] for this ticker.
    #[inline]
    pub fn runner_log(&self) -> RunnerTickerLog {
        RunnerMode::Simulated.with_ticker(self.ticker)
    }

    /// Portfolio slot inputs for this bar (then call [`PortfolioSlotSizing::slot_sizing`] with equity).
    #[inline]
    pub fn portfolio_slots(&self, open_positions: usize) -> PortfolioSlotSizing {
        PortfolioSlotSizing {
            slots_used: open_positions,
            max_positions: self.max_pos,
            per_position_pct_div: self.per_position_pct_div,
        }
    }

    #[inline]
    pub fn slot_sizing(&self, open_positions: usize) -> SlotSizing {
        self.portfolio_slots(open_positions).slot_sizing(self.portfolio_equity)
    }

    #[inline]
    pub fn entry_open_log(&self) -> EntryOpenDailyLog {
        self.runner_log()
            .with_daily_entry_cap(self.config.max_entries_per_day)
    }

    /// [`EntryPrepareCtx`] for this strategy config and the given slot (live/backtest DRY).
    #[inline]
    pub fn entry_prepare_ctx<'a>(
        &self,
        slot: &'a SlotSizing,
    ) -> EntryPrepareCtx<'a, '_> {
        EntryPrepareCtx::new(slot, self.config)
    }

    /// Whether deferred entry / exit lines are printed (`verbosity >= 2`).
    #[inline]
    pub fn entry_exit_log_verbose(&self) -> bool {
        self.verbosity >= 2
    }

    /// Bar open plus configured entry slippage.
    #[inline]
    pub fn open_price_with_slippage(&self, bar_open: f64) -> f64 {
        bar_open + self.bc.slippage()
    }

    #[inline]
    pub fn commission_min(&self) -> f64 {
        self.bc.commission_min
    }
}

/// Result of a 1-min SLTP check. `Some(cash_returned)` if position was closed.
pub type SltpOutcome = Option<f64>;

impl TickerState {
    #[inline]
    fn record_exit_with_ctx(&mut self, trade: Trade, bar_time_sec: i64, ctx: &ExecContext) {
        self.record_exit(ChartExitRecord { trade, bar_time_sec, equity: ctx.portfolio_equity, starting_capital: ctx.starting_capital });
    }

    pub fn tick_pending_timers(&mut self) {
        if let Some(ref mut pe) = self.pending_entry {
            pe.bars_left -= 1;
        }
    }

    /// Fill a deferred entry at bar.open. Returns cash consumed if filled.
    pub fn fill_pending_entry(
        &mut self,
        bar: &OhlcBar,
        cash: f64,
        open_positions: usize,
        ctx: &ExecContext,
    ) -> Option<f64> {
        let fill_ready = self.pending_entry.as_ref().is_some_and(|pe| pe.bars_left <= 0);
        if !fill_ready {
            return None;
        }
        if self.position.is_some() {
            self.pending_entry = None;
            return None;
        }

        let pe = self.pending_entry.take().expect("fill_pending_entry called without pending entry");
        let regime = pe.regime();
        let entry_price = ctx.open_price_with_slippage(bar.open);
        let Some((_inputs, prep)) = ctx
            .slot_sizing(open_positions)
        .try_prepare_bundle(
            |slot| pe.prepare_bundle_at_trade_price(entry_price, &ctx.entry_prepare_ctx(slot)),
            || {
                self.engine.undo_commit();
                println!("{}", ctx.runner_log().reject_stops_failed_line(&pe.prepare.signal));
            },
        ) else {
            return None;
        };

        let sh = prep.shares.to_f64();
        if sh * entry_price + ctx.commission_min() > cash {
            return None;
        }

        let pos = Position::open_with_prep(
            &prep,
            pe.prepare.signal,
            bar.timestamp,
            bar.open,
            pe.spike_bar,
            regime.atr,
            ctx.bc,
        );
        let consumed = pos.entry_cost + pos.entry_commission;
        self.engine.on_entry(&mut self.daily, pos.entry_price);
        if ctx.entry_exit_log_verbose() {
            println!(
                "{}",
                ctx.entry_open_log().format_line_quote(
                    EntryOpenLogKind::DeferredFill {
                        atr_regime_ratio: regime.atr_regime_ratio,
                    },
                    self.daily.entries,
                    pe.prepare.signal,
                    &prep.open_line_quote(&PreparedOpenLineCtx::new(
                        pos.entry_price,
                        pe.diag.entry_reason.as_str(),
                    )),
                ),
            );
        }
        self.push_entry_marker(bar.timestamp.timestamp(), pe.prepare.signal);
        self.position_diag = Some(pe.diag);
        self.position = Some(pos);
        Some(consumed)
    }

    /// Check stop-loss / take-profit on a 1-min bar.
    pub fn check_sltp_1m(
        &mut self,
        bar: &OhlcBar,
        gex_map: Option<&HashMap<i64, GexProfile>>,
        ctx: &ExecContext,
    ) -> SltpOutcome {
        if let Some(pos) = self.position.as_mut() {
            pos.bars_held += 1;
            if bar.high > pos.max_high { pos.max_high = bar.high; }
        }
        let hit = self.position.as_ref().and_then(|pos| pos.check_sltp(bar));
        let (price, reason) = hit?;

        Some(self.close_at(bar, price, reason, gex_map, ctx))
    }

    /// Close the current position at `price`. Returns cash returned.
    pub fn close_at(
        &mut self,
        bar: &OhlcBar,
        price: f64,
        reason: &str,
        gex_map: Option<&HashMap<i64, GexProfile>>,
        ctx: &ExecContext,
    ) -> f64 {
        let pos = self.position.as_ref().expect("close_at called without position");
        let entry_price = pos.entry_price;
        let (mut trade, returned) = pos.close(&PositionCloseCtx {
            exit_time: bar.timestamp,
            raw_exit_price: price,
            exit_reason: reason,
            ticker: ctx.ticker,
            bc: ctx.bc,
        });
        let sltp_gex = gex_map.and_then(|m| m.get(&ts_key(&bar.timestamp)));
        trade.diagnostics = self.position_diag.as_ref().map(|d| d.finalize(sltp_gex, entry_price));
        if ctx.entry_exit_log_verbose() {
            println!(
                "{}",
                ctx.runner_log().format_close_line(&CloseLineQuote::new(
                    reason,
                    trade.exit_price,
                    trade.net_pnl,
                    trade.return_pct,
                    Some(trade.max_runup_atr),
                )),
            );
        }
        self.record_exit_with_ctx(trade, bar.timestamp.timestamp(), ctx);
        returned
    }

    /// Close the current position at market (bar close). Returns cash returned if position exists.
    pub fn close_at_market(
        &mut self,
        bar: &OhlcBar,
        reason: &str,
        gex_map: Option<&HashMap<i64, GexProfile>>,
        ctx: &ExecContext,
    ) -> Option<f64> {
        self.position.as_ref()?;
        Some(self.close_at(bar, bar.close, reason, gex_map, ctx))
    }
}
