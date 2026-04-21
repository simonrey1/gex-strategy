use anyhow::Result;
use std::sync::Arc;

use crate::broker::ibkr::OrderIdGen;
use crate::broker::types::BracketOrderIds;
use crate::config::Ticker;
use crate::strategy::shared::PortfolioSlotSizing;
use crate::types::ToF64;

use super::bracket_legs::BracketLegs;
use super::order_fill::SharedFillState;

/// Thin wrapper around `ibapi::Client` that groups all order-placement methods.
/// Uses our own `OrderIdGen` to avoid ibapi's internal counter, which can be
/// stale after IB Gateway restarts (paper trading returns nextValidId=1).
pub struct OrderClient<'a> {
    pub client: &'a ibapi::Client,
    id_gen: Arc<OrderIdGen>,
}

impl<'a> OrderClient<'a> {
    pub fn new(client: &'a ibapi::Client, id_gen: Arc<OrderIdGen>) -> Self {
        Self { client, id_gen }
    }

    pub async fn cancel_bracket_quietly(&self, bracket: &Option<BracketOrderIds>) {
        if let Some(bi) = bracket {
            if let Err(e) = self.client.cancel_order(bi.stop_loss_id, "").await {
                eprintln!("[live] Failed to cancel SL order {}: {:?}", bi.stop_loss_id, e);
            }
            if let Err(e) = self.client.cancel_order(bi.take_profit_id, "").await {
                eprintln!("[live] Failed to cancel TP order {}: {:?}", bi.take_profit_id, e);
            }
        }
    }

    pub async fn place_market_sell(&self, ticker: Ticker, shares: u32) -> Result<()> {
        self.place_market_order(ticker, shares, false).await
    }

    pub async fn place_market_buy(&self, ticker: Ticker, shares: u32) -> Result<()> {
        self.place_market_order(ticker, shares, true).await
    }

    async fn place_market_order(&self, ticker: Ticker, shares: u32, is_buy: bool) -> Result<()> {
        use ibapi::prelude::*;
        let contract = Contract::stock(ticker.as_str()).build();
        let qty = shares.to_f64();
        let builder = self.client.order(&contract);
        let order = if is_buy {
            builder.buy(qty).market().build_order()
        } else {
            builder.sell(qty).market().build_order()
        }.map_err(|e| anyhow::anyhow!("build market order failed: {:?}", e))?;

        let oid = self.id_gen.next();
        self.client.submit_order(oid, &contract, &order).await
            .map_err(|e| anyhow::anyhow!("market {} failed (oid={}): {:?}", if is_buy { "buy" } else { "sell" }, oid, e))?;
        let label = if is_buy { "BUY" } else { "SELL" };
        let reason = if is_buy { "cover short" } else { "emergency/exit" };
        println!("[ibkr] MARKET {} {} x{} oid={} ({})", label, ticker, shares, oid, reason);
        Ok(())
    }

    /// Place GTC SL + TP orders for a position. Used at entry and to re-place
    /// brackets after a restart when DAY orders have expired overnight.
    pub async fn place_gtc_bracket(&self, legs: &BracketLegs) -> Result<BracketOrderIds> {
        use ibapi::prelude::*;
        let contract = Contract::stock(legs.ticker.as_str()).build();

        let rounded_sl = crate::types::round_cents(legs.sl_price);
        let rounded_tp = crate::types::round_cents(legs.tp_price);
        let qty = legs.shares.to_f64();

        let tp_order = self.client.order(&contract)
            .sell(qty)
            .good_till_cancel()
            .limit(rounded_tp)
            .build_order()
            .map_err(|e| anyhow::anyhow!("build GTC TP order failed: {:?}", e))?;

        let tp_oid = self.id_gen.next();
        self.client.submit_order(tp_oid, &contract, &tp_order).await
            .map_err(|e| anyhow::anyhow!("GTC TP order submit failed (oid={}): {:?}", tp_oid, e))?;

        let sl_order = self.client.order(&contract)
            .sell(qty)
            .good_till_cancel()
            .stop(rounded_sl)
            .build_order()
            .map_err(|e| anyhow::anyhow!("build GTC SL order failed: {:?}", e))?;

        let sl_oid = self.id_gen.next();
        self.client.submit_order(sl_oid, &contract, &sl_order).await
            .map_err(|e| anyhow::anyhow!("GTC SL order submit failed (oid={}): {:?}", sl_oid, e))?;

        Ok(BracketOrderIds {
            parent_id: 0,
            stop_loss_id: sl_oid,
            take_profit_id: tp_oid,
        })
    }

    /// Cancel the old stop order and place a new one at the updated price.
    /// Checks fill state to avoid placing a naked stop if the old SL already triggered.
    /// If the new order fails, restores a stop at `old_sl_price` so we're never unprotected.
    /// Returns `(order_id, actual_sl_price)` — the price may be `old_sl_price` if fallback was used.
    pub async fn replace_stop(
        &self,
        ticker: Ticker,
        shares: u32,
        old_sl_id: i32,
        new_sl_price: f64,
        old_sl_price: f64,
        fill_state: &SharedFillState,
    ) -> Result<(i32, f64)> {
        use ibapi::prelude::*;

        {
            let fills = crate::types::lock_or_recover(fill_state);
            if fills.contains_key(&old_sl_id) {
                return Err(anyhow::anyhow!("old SL {} already filled — skip replace", old_sl_id));
            }
        }

        let contract = Contract::stock(ticker.as_str()).build();

        if let Err(e) = self.client.cancel_order(old_sl_id, "").await {
            eprintln!("[live-{}] SL cancel failed (may have filled): {:?}", ticker, e);
            return Err(anyhow::anyhow!("cancel old SL failed: {:?}", e));
        }

        {
            let fills = crate::types::lock_or_recover(fill_state);
            if fills.contains_key(&old_sl_id) {
                return Err(anyhow::anyhow!("old SL {} filled during cancel — skip replace", old_sl_id));
            }
        }

        let rounded_sl = crate::types::round_cents(new_sl_price);
        let qty = shares.to_f64();

        let order = self.client.order(&contract)
            .sell(qty)
            .good_till_cancel()
            .stop(rounded_sl)
            .build_order()
            .map_err(|e| anyhow::anyhow!("build stop order failed: {:?}", e))?;

        let oid = self.id_gen.next();
        match self.client.submit_order(oid, &contract, &order).await {
            Ok(()) => Ok((oid, rounded_sl)),
            Err(e) => {
                eprintln!(
                    "[live-{}] New SL @ ${:.2} failed (oid={}), restoring @ ${:.2}: {:?}",
                    ticker, new_sl_price, oid, old_sl_price, e
                );
                let fallback_order = self.client.order(&contract)
                    .sell(qty)
                    .good_till_cancel()
                    .stop(old_sl_price)
                    .build_order()
                    .map_err(|e2| anyhow::anyhow!("build fallback stop failed: {:?}", e2))?;
                let fallback_oid = self.id_gen.next();
                self.client.submit_order(fallback_oid, &contract, &fallback_order).await
                    .map_err(|e2| anyhow::anyhow!(
                        "CRITICAL: both new and fallback SL failed: {:?} / {:?}", e, e2
                    ))?;
                Ok((fallback_oid, old_sl_price))
            }
        }
    }

    pub async fn fetch_ticker_bars(
        &self,
        ticker: Ticker,
        ts: &mut crate::live::ticker_state::LiveTickerState,
    ) -> Vec<crate::types::OhlcBar> {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_millis(crate::live::live_poll_policy::LivePollPolicy::FETCH_TIMEOUT_MS),
            crate::broker::ibkr::fetch_ibkr_intraday_bars(self.client, ticker),
        ).await;

        match result {
            Ok(Ok(bars)) => {
                ts.consecutive_failures = 0;
                bars
            }
            err => {
                let msg = match &err {
                    Ok(Err(e)) => format!("{:?}", e),
                    Err(e) => format!("timeout: {:?}", e),
                    Ok(Ok(_)) => "unexpected Ok(Ok(_)) in error arm".to_string(),
                };
                ts.consecutive_failures += 1;
                let backoff = crate::live::live_poll_policy::LivePollPolicy::backoff_ms(ts.consecutive_failures);
                eprintln!(
                    "[live-{}] Bar fetch failed (attempt {}/{}): {}",
                    ticker, ts.consecutive_failures, crate::live::live_poll_policy::LivePollPolicy::MAX_CONSECUTIVE_FAILURES, msg
                );
                if crate::live::live_poll_policy::LivePollPolicy::should_emergency_close(ts.consecutive_failures, ts.position.is_some()) {
                    eprintln!(
                        "[live-{}] {} consecutive failures in position — EMERGENCY CLOSE",
                        ticker, crate::live::live_poll_policy::LivePollPolicy::MAX_CONSECUTIVE_FAILURES
                    );
                    ts.force_close(self, "fetch_failures").await;
                }
                crate::live::log_debug!("[live-{}] Retrying in {}s...", ticker, backoff / 1000);
                Vec::new()
            }
        }
    }

    pub async fn rank_and_execute_entries(
        &self,
        candidates: &mut Vec<crate::live::ticker_state::LiveEntryCandidate>,
        states: &mut std::collections::HashMap<Ticker, crate::live::ticker_state::LiveTickerState>,
        max_pos: usize,
        per_position_div: f64,
    ) {
        crate::strategy::engine::rank_and_dedup(candidates);

        let mut open_positions = states.values().filter(|s| s.slot_held()).count();

        if candidates.len() > 1 {
            let avail = crate::strategy::engine::SlotSizing::remaining_slots(max_pos, open_positions);
            crate::live::log_debug!(
                "[live] {} entry candidates, {} slots available (open={})",
                candidates.len(), avail, open_positions
            );
        }

        for candidate in candidates.drain(..) {
            let ts = states.get_mut(&candidate.ticker).expect("ticker missing from states");
            if ts
                .execute_entry(
                    &candidate,
                    self,
                    PortfolioSlotSizing {
                        slots_used: open_positions,
                        max_positions: max_pos,
                        per_position_pct_div: per_position_div,
                    },
                )
                .await
            {
                open_positions += 1;
            }
        }
    }
}
