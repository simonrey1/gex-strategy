use std::sync::Arc;

use crate::broker::types::BracketOrderIds;
use crate::config::{StrategyConfig, Ticker};
use crate::strategy::engine::{DailyState, EntryCandidateData, StrategyEngine};
use crate::strategy::shared::{
    CloseLineQuote, EntryOpenDailyLog, EntryOpenLogKind,
    EntryPrepareCtx, ExitPnlInputs, GexPipelineBar, PortfolioSlotSizing, RunnerMode, RunnerTickerLog,
    SizedEntryBrackets, SlotSizing,
};
use crate::strategy::wall_trail::WallTrailOutcome;
use crate::strategy::hurst::HurstTracker;
use crate::strategy::wall_smoother::WallSmoother;
use crate::types::{F64Trunc, OhlcBar, Signal, ToF64, TradeSignal};

use super::dashboard::{TickerHealth, TickerIndicators};
use super::log_debug;
use super::orders::{take_fill, OrderClient, SharedFillState};
use super::reconcile::ReconcileAction;
use super::state::{LivePosition, RunnerSnapshot};
use super::trade_log::log_exit_trade;
pub use super::live_entry_candidate::LiveEntryCandidate;

// ─── Per-ticker mutable state ───────────────────────────────────────────────

pub struct LiveTickerState {
    pub ticker: Ticker,
    pub engine: StrategyEngine,
    pub config: Arc<StrategyConfig>,
    pub position: Option<LivePosition>,
    pub bracket_ids: Option<BracketOrderIds>,
    pub daily: DailyState,
    pub last_processed_ms: i64,
    pub spot_price: f64,
    pub last_fresh_data_ms: u64,
    pub consecutive_failures: u32,
    pub last_known_equity: f64,
    pub had_new_data: bool,
    pub hurst: HurstTracker,
    pub wall_smoother: WallSmoother,
}

impl LiveTickerState {
    /// Live runner always uses [`RunnerMode::External`] for this ticker.
    #[inline]
    fn runner_log(&self) -> RunnerTickerLog {
        RunnerMode::External.with_ticker(self.ticker)
    }

    #[inline]
    fn entry_open_log(&self) -> EntryOpenDailyLog {
        self.runner_log()
            .with_daily_entry_cap(self.config.max_entries_per_day)
    }

    #[inline]
    fn entry_prepare_ctx<'a>(
        &self,
        slot: &'a SlotSizing,
    ) -> EntryPrepareCtx<'a, '_> {
        EntryPrepareCtx::new(slot, self.config.as_ref())
    }

    /// Open position consumes a slot (live does not model pending-until-fill like backtest).
    #[inline]
    pub fn slot_held(&self) -> bool {
        self.position.is_some()
    }

    /// [`StrategyEngine::process_gex_bar_pipeline`] with this ticker’s smoother, hurst, and position.
    /// Clones the per-ticker [`Arc`] so `pipe` does not borrow `self.config` across `&mut self`.
    #[inline]
    pub fn run_gex_bar_pipeline(
        &mut self,
        pipe: GexPipelineBar<'_>,
    ) -> (TradeSignal, WallTrailOutcome, Option<EntryCandidateData>) {
        let cfg = Arc::clone(&self.config);
        self.engine.process_gex_bar_pipeline(
            &mut self.wall_smoother,
            &mut self.hurst,
            self.position.as_mut(),
            pipe,
            cfg.as_ref(),
            &self.daily,
        )
    }

    pub fn save_snapshot(&self) {
        RunnerSnapshot {
            engine: &self.engine,
            position: &self.position,
            bracket_ids: &self.bracket_ids,
            last_processed_ms: self.last_processed_ms,
            daily: &self.daily,
        }.save(self.ticker);
    }

    /// Cancel bracket orders, market-sell shares, log the exit, update engine
    /// state, and persist the snapshot. Single path for all forced closures.
    pub async fn force_close(&mut self, orders: &OrderClient<'_>, reason: &str) {
        let shares = match &self.position {
            Some(pos) => pos.shares,
            None => return,
        };
        orders.cancel_bracket_quietly(&self.bracket_ids).await;
        if let Err(e) = orders.place_market_sell(self.ticker, shares).await {
            eprintln!(
                "{} CRITICAL: emergency sell failed: {:?} — keeping position for reconciliation",
                self.runner_log().tag(), e
            );
            return;
        }
        self.apply_exit(reason, self.spot_price);
        self.save_snapshot();
    }

    pub async fn refresh_equity(&mut self, ibkr_client: &ibapi::Client) {
        if let Some(eq) = super::equity::AccountEquity::fetch(ibkr_client).await {
            if let Some(usd) = eq.usd_available {
                self.last_known_equity = usd;
            }
        }
    }

    /// Record an exit: log, update engine/daily state, clear position.
    pub fn apply_exit(&mut self, reason: &str, exit_price: f64) {
        let pos = self.position.as_ref().expect("apply_exit called without position");
        let (pnl, return_pct) =
            ExitPnlInputs::new(pos.entry_price, exit_price, pos.shares).gross_pnl_and_return_pct();
        log_exit_trade(self.ticker, pos, reason, exit_price, pnl, return_pct, self.last_known_equity);
        println!(
            "{}",
            self.runner_log().format_close_line(&CloseLineQuote::new(
                reason,
                exit_price,
                pnl,
                return_pct,
                None,
            )),
        );
        self.engine
            .on_exit(&mut self.daily, pnl, self.last_known_equity);
        self.position = None;
        self.bracket_ids = None;
        self.had_new_data = true;
    }

    pub fn current_indicators(&self) -> Option<TickerIndicators> {
        self.engine
            .last_indicator_values
            .as_ref()
            .map(TickerIndicators::from_values)
    }

    /// Handle a reconciliation action from IBKR position/order polling.
    pub fn apply_reconcile(&mut self, action: ReconcileAction) {
        match action {
            ReconcileAction::Consistent => {}

            ReconcileAction::PositionGone => {
                println!(
                    "[reconcile-{}] Position closed externally (IBKR flat)",
                    self.ticker,
                );
                self.apply_exit("reconcile_ibkr_flat", self.spot_price);
            }

            ReconcileAction::AdoptPosition { shares, avg_cost, sl_order_id, sl_price, tp_order_id, tp_price, extra_stop_ids: _ } => {
                println!(
                    "[reconcile-{}] Adopting IBKR position: {} shares @ ${:.2} | SL={:?} TP={:?}",
                    self.ticker, shares, avg_cost, sl_order_id, tp_order_id
                );
                self.position = Some(LivePosition {
                    holding: self.engine.signal_state.holding,
                    shares: shares.trunc_u32(),
                    entry_price: avg_cost,
                    entry_time: chrono::Utc::now().to_rfc3339(),
                    stop_loss: sl_price,
                    take_profit: tp_price,
                    entry_atr: 0.0,
                    highest_put_wall: 0.0,
                    highest_close: avg_cost,
                    hurst_exhaust_bars: 0,
                });
                if let (Some(sl), Some(tp)) = (sl_order_id, tp_order_id) {
                    self.bracket_ids = Some(BracketOrderIds {
                        parent_id: 0,
                        stop_loss_id: sl,
                        take_profit_id: tp,
                    });
                }
                if self.engine.signal_state.holding.is_flat() {
                    // Unknown signal → VF: no IV-based exits that could trigger a
                    // market sell while IBKR brackets are still live.
                    self.engine.signal_state.holding = Signal::LongVannaFlip;
                }
                self.engine.signal_state.entry_price = avg_cost;
                self.had_new_data = true;
            }

            ReconcileAction::BracketStale { sl_order_id, sl_price, tp_order_id, tp_price, extra_stop_ids: _ } => {
                log_debug!(
                    "[reconcile-{}] Updating stale bracket IDs: SL={:?} TP={:?}",
                    self.ticker, sl_order_id, tp_order_id
                );
                match (sl_order_id, tp_order_id) {
                    (Some(sl), Some(tp)) => {
                        if let Some(ref mut pos) = self.position {
                            pos.stop_loss = sl_price;
                            pos.take_profit = tp_price;
                        }
                        self.bracket_ids = Some(BracketOrderIds {
                            parent_id: 0,
                            stop_loss_id: sl,
                            take_profit_id: tp,
                        });
                    }
                    _ => {
                        eprintln!(
                            "[reconcile-{}] WARNING: brackets expired (SL={:?}, TP={:?}) — will re-place",
                            self.ticker, sl_order_id, tp_order_id
                        );
                        self.bracket_ids = None;
                    }
                }
                self.had_new_data = true;
            }

            ReconcileAction::OrphanedOrders { order_ids } => {
                println!(
                    "[reconcile-{}] Orphaned orders detected (no position): {:?} — cancelling in runner",
                    self.ticker, order_ids
                );
            }
        }
    }

    /// Check IBKR bracket fills (SL/TP hit between polls).
    /// SL/TP fill → close position and cancel the counterpart order.
    pub async fn check_bracket_fills(
        &mut self,
        fill_state: &SharedFillState,
        ibkr_client: &ibapi::Client,
    ) {
        let (_shares, sl_id, tp_id) = match (&self.position, &self.bracket_ids) {
            (Some(pos), Some(bi)) => (pos.shares, bi.stop_loss_id, bi.take_profit_id),
            _ => return,
        };

        let sl_fill = take_fill(fill_state, sl_id);
        let tp_fill = take_fill(fill_state, tp_id);
        let is_sl = sl_fill.is_some();
        if let Some(fill) = sl_fill.or(tp_fill) {
            self.refresh_equity(ibkr_client).await;
            let exit_price = if fill.avg_price > 0.0 { fill.avg_price } else { self.spot_price };

            let cancel_id = if is_sl { tp_id } else { sl_id };
            if let Err(e) = ibkr_client.cancel_order(cancel_id, "").await {
                eprintln!("[live] Failed to cancel counterpart order {}: {:?}", cancel_id, e);
            }

            let reason = if is_sl { "stop_loss (IBKR)" } else { "take_profit (IBKR)" };
            self.apply_exit(reason, exit_price);
        }
    }

    /// Build a health snapshot for the dashboard.
    pub fn health_snapshot(
        &self,
        gex_error: &Option<String>,
        last_bar: Option<&OhlcBar>,
        bars_today: u32,
    ) -> TickerHealth {
        let ind = self.current_indicators();
        let ws = if ind.is_some() { None } else { Some("Need more history".to_string()) };
        let ticker_error = match (gex_error, self.consecutive_failures > 0) {
            (Some(ge), _) => Some(format!("GEX: {}", ge)),
            (None, true) => Some(format!("{} consecutive fetch failures", self.consecutive_failures)),
            _ => None,
        };
        TickerHealth {
            last_poll_ms: crate::types::now_ms(),
            position: self.position.is_some(),
            signal: Some(self.engine.signal_state.holding),
            spot_price: self.spot_price,
            last_bar_time: last_bar.map(|b| b.timestamp.to_rfc3339()),
            bars_today,
            consecutive_failures: self.consecutive_failures,
            equity: self.last_known_equity,
            last_error: ticker_error,
            indicators: ind,
            warmup_status: ws,
        }
    }

    /// Size and execute an entry via IBKR bracket order. Returns true if placed.
    pub async fn execute_entry(
        &mut self,
        candidate: &LiveEntryCandidate,
        orders: &OrderClient<'_>,
        portfolio_slots: PortfolioSlotSizing,
    ) -> bool {
        self.refresh_equity(orders.client).await;
        let d = &candidate.data;
        let rr = self.engine.atr_regime_ratio();
        let Some((inputs, prep)) = portfolio_slots
            .slot_sizing(self.last_known_equity)
        .try_prepare_bundle(
            |slot| d.entry_prepare_bundle(rr, &self.entry_prepare_ctx(slot)),
            || eprintln!("{}", self.runner_log().reject_stops_failed_line(&d.signal)),
        ) else {
            return false;
        };
        let shares = prep.shares;
        let sl = prep.stop_loss;
        let tp = prep.take_profit;

        let qty = shares.to_f64();
        let buy_result = orders.client
            .order(&ibapi::prelude::Contract::stock(self.ticker.as_str()).build())
            .buy(qty)
            .market()
            .submit()
            .await;

        let parent_id: i32 = match buy_result {
            Ok(id) => id.into(),
            Err(e) => {
                eprintln!("{} Market buy failed: {:?}", self.runner_log().tag(), e);
                return false;
            }
        };

        let legs = super::orders::BracketLegs {
            ticker: self.ticker, shares, sl_price: sl, tp_price: tp,
        };
        let bracket_result = orders.place_gtc_bracket(&legs).await;

        match bracket_result {
            Ok(ids) => {
                let new_bracket = BracketOrderIds {
                    parent_id,
                    stop_loss_id: ids.stop_loss_id,
                    take_profit_id: ids.take_profit_id,
                };
                let live_pos = LivePosition {
                    holding: inputs.signal,
                    shares,
                    entry_price: d.entry_price,
                    entry_time: chrono::Utc::now().to_rfc3339(),
                    stop_loss: sl,
                    take_profit: tp,
                    entry_atr: d.atr(),
                    highest_put_wall: 0.0,
                    highest_close: d.entry_price,
                    hurst_exhaust_bars: 0,
                };

                self.engine.commit_entry(inputs.signal, d.entry_price);
                self.engine.on_entry(&mut self.daily, d.entry_price);
                super::trade_log::log_entry_trade(self.ticker, &live_pos, &d.reason, self.last_known_equity);
                println!(
                    "{}",
                    self.entry_open_log().format_line_quote(
                        EntryOpenLogKind::SignalCommit {
                            equity: self.last_known_equity,
                        },
                        self.daily.entries,
                        inputs.signal,
                        &d.open_line_quote(&SizedEntryBrackets { shares: shares, stop_loss: sl, take_profit: tp }),
                    ),
                );

                self.bracket_ids = Some(new_bracket.clone());
                self.position = Some(live_pos);

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let ibkr_orders = super::reconcile::fetch_ibkr_orders(orders.client).await;
                let sym_orders = ibkr_orders
                    .as_ref()
                    .and_then(|m| m.get(self.ticker.as_str()));
                let (found_sl, found_tp, _extra) = sym_orders
                    .map(|o| super::reconcile::find_bracket_orders(o))
                    .unwrap_or((None, None, vec![]));
                if ibkr_orders.is_none() {
                    eprintln!(
                        "{} Could not verify bracket (orders fetch failed) — keeping position",
                        self.runner_log().tag(),
                    );
                } else if found_sl.is_none() || found_tp.is_none() {
                    eprintln!(
                        "{} BRACKET INCOMPLETE (SL={} TP={}) — emergency close",
                        self.runner_log().tag(),
                        if found_sl.is_some() { "ok" } else { "MISSING" },
                        if found_tp.is_some() { "ok" } else { "MISSING" },
                    );
                    self.force_close(orders, "bracket_rejected").await;
                    return false;
                }

                true
            }
            Err(e) => {
                eprintln!("{} GTC bracket failed after buy: {:?} — emergency sell", self.runner_log().tag(), e);
                if let Err(sell_err) = orders.place_market_sell(self.ticker, shares).await {
                    eprintln!("{} CRITICAL: emergency sell also failed: {:?}", self.runner_log().tag(), sell_err);
                }
                false
            }
        }
    }
}
