use anyhow::Result;
use std::collections::HashMap;
use crate::broker::ibkr::IbkrBroker;
use crate::broker::types::Broker as _;
use crate::config::bar_interval;
use crate::config::{StrategyConfig, Ticker};
use crate::strategy::engine::WallTrailOutcome;
use crate::strategy::shared::GexPipelineBar;
use crate::data::thetadata_live::get_live_gex_profile;
use crate::types::{AsLenU32, F64Trunc, OhlcBar};

use super::dashboard::update_health;
use super::init::{init_ticker_states, LiveContext};
use super::log_debug;
use super::nyse_session::NyseSession;
use super::orders::OrderClient;
use super::reconcile;
use super::reconcile::{fetch_ibkr_orders, fetch_ibkr_positions};
use super::ticker_state::LiveEntryCandidate;

pub async fn run_live(
    tickers: &[Ticker],
    config: &StrategyConfig,
    broker: IbkrBroker,
    server_cfg: super::auth::ServerConfig,
) -> Result<()> {
    let port = server_cfg.port;
    let (ctx, mut broker) = LiveContext::setup(tickers, config, broker, server_cfg).await?;
    let ibkr_client = &ctx.ibkr_client;
    let orders = OrderClient::new(ibkr_client);

    let mut states = init_ticker_states(
        tickers, config, ibkr_client, &ctx.health_state,
        ctx.initial_equity, &ctx.spot_prices,
    ).await?;

    let interval = crate::config::BAR_INTERVAL_MINUTES;
    let (max_pos, per_position_div) = config.slot_params();
    let poll_ms = bar_interval::poll_interval_ms(interval);

    println!("[live] Live system started for {:?}", tickers);
    println!("[live] ─────────────────────────────────────────────");
    println!("[live] Dashboard: http://localhost:{}", port);
    println!("[live] Press Ctrl+C to stop.");
    println!("[live] ─────────────────────────────────────────────");

    let shutdown = ctx.shutdown.clone();
    let shutdown_flag = shutdown.clone();
    if let Err(e) = unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGINT, move || {
            if shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("\n[live] Forced exit.");
                std::process::exit(1);
            }
            shutdown_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            eprintln!("\n[live] Shutting down gracefully (Ctrl+C again to force)...");
        })
    } {
        eprintln!("[live] SIGINT handler registration failed: {} — Ctrl+C may not work", e);
    }

    // One-time cleanup: cancel all stale orders from previous gateway sessions.
    // After a gateway restart, old orders are still active server-side but
    // unmanageable via individual cancel_order (wrong client/session).
    // global_cancel clears them so we can re-place fresh brackets.
    if let Err(e) = ibkr_client.global_cancel().await {
        eprintln!("[live] global_cancel failed: {:?} (non-fatal)", e);
    } else {
        println!("[live] global_cancel issued — clearing stale orders from previous sessions");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    let mut current_day = NyseSession::et_date_str(&chrono::Utc::now());
    let freshness_timeout_ms = poll_ms * 3;
    let mut ibkr_down_since: Option<tokio::time::Instant> = None;
    let mut exit_reason: Option<&str> = None;
    log_debug!("[live] Entering centralized poll loop...");

    // ── Main loop ────────────────────────────────────────────────────────

    loop {
        let now = chrono::Utc::now();

        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        // ── IBKR connection watchdog ──────────────────────────────────
        if !ibkr_client.is_connected() {
            let instant_now = tokio::time::Instant::now();
            let since = ibkr_down_since.get_or_insert(instant_now);
            let secs = instant_now.duration_since(*since).as_secs();
            if secs >= 60 {
                eprintln!(
                    "[live] IBKR disconnected for {}s — restarting process for reconnection",
                    secs
                );
                exit_reason = Some("ibkr_disconnect");
                break;
            }
            eprintln!(
                "[live] IBKR disconnected ({}s) — waiting for auto-reconnect...",
                secs
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            continue;
        } else if ibkr_down_since.take().is_some() {
            println!("[live] IBKR reconnected");
        }

        // Day rollover
        let today = NyseSession::et_date_str(&now);
        if today != current_day {
            if !current_day.is_empty() {
                log_debug!("[live] New trading day: {} (was {})", today, current_day);
            }
            current_day = today;
            for ts in states.values_mut() {
                ts.daily.reset();
            }
        }

        if NyseSession::is_closed_all_day(&now) {
            log_debug!("[live] Market closed today — sleeping 1 hour...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            continue;
        }
        if !NyseSession::is_open(&now) {
            NyseSession::wait_until_open(&now).await;
            continue;
        }

        let just_closed: std::collections::HashSet<Ticker> = std::collections::HashSet::new();

        // ── IBKR reconciliation ──────────────────────────────────────
        let ibkr_positions = fetch_ibkr_positions(ibkr_client).await;
        let ibkr_orders = fetch_ibkr_orders(ibkr_client).await;

        if let (Some(ref positions), Some(ref open_orders)) = (&ibkr_positions, &ibkr_orders) {
            // Safety check: if we think we have local positions but IBKR returns empty,
            // something may be wrong with the fetch — skip orphaned order cancellation.
            let local_position_count = states.values().filter(|ts| ts.slot_held()).count();
            let positions_fetch_suspect = local_position_count > 0 && positions.is_empty();
            if positions_fetch_suspect {
                eprintln!(
                    "[reconcile] WARNING: {} local positions but IBKR returned empty — skipping orphaned order cleanup",
                    local_position_count
                );
            }

            for &ticker in tickers {
                if just_closed.contains(&ticker) {
                    log_debug!(
                        "[reconcile-{}] Skipping — just force-closed this cycle",
                        ticker
                    );
                    continue;
                }
                let ts = states.get_mut(&ticker).expect("ticker missing from states");
                let local_view = reconcile::LocalTickerView {
                    ticker,
                    has_position: ts.position.is_some(),
                    bracket_sl: ts.bracket_ids.as_ref().map(|b| b.stop_loss_id),
                    bracket_tp: ts.bracket_ids.as_ref().map(|b| b.take_profit_id),
                };
                let action = local_view.reconcile(positions, open_orders);

                // Cancel orphaned orders before updating state (skip if positions fetch is suspect)
                if let super::reconcile::ReconcileAction::OrphanedOrders { ref order_ids } = action {
                    if positions_fetch_suspect {
                        eprintln!(
                            "[reconcile-{}] Skipping orphaned order cancellation (positions fetch suspect)",
                            ticker
                        );
                    } else {
                        for &oid in order_ids {
                            println!("[reconcile-{}] Cancelling orphaned order {}", ticker, oid);
                            if let Err(e) = ibkr_client.cancel_order(oid, "").await {
                                eprintln!("[reconcile-{}] Failed to cancel order {}: {:?}", ticker, oid, e);
                            }
                        }
                    }
                }

                // Extra STP orders from previous wall-trail ratchets.
                // Always force a full bracket re-place when extras exist: IBKR's
                // cancel_order returns Ok even when the order belongs to a prior
                // client_id (the 10147 error arrives asynchronously). The re-place
                // path cancels ALL sell orders for this ticker, then places a clean
                // SL+TP pair owned by the current session.
                let has_extras = match &action {
                    super::reconcile::ReconcileAction::AdoptPosition { extra_stop_ids, .. }
                    | super::reconcile::ReconcileAction::BracketStale { extra_stop_ids, .. } => !extra_stop_ids.is_empty(),
                    _ => false,
                };

                ts.apply_reconcile(action);

                if has_extras {
                    println!(
                        "[reconcile-{}] Stale ratchet STP orders detected — forcing bracket re-place",
                        ticker
                    );
                    ts.bracket_ids = None;
                }

                if let (Some(pos), None) = (&ts.position, &ts.bracket_ids) {
                    if pos.stop_loss > 0.0 && pos.take_profit > 0.0 {
                        // Cancel all existing SELL orders before re-placing to avoid duplicates
                        if let Some(ords) = open_orders.get(ticker.as_str()) {
                            for o in ords {
                                if o.action == "Sell" {
                                    println!("[reconcile-{}] Cancelling pre-existing order {} before re-place", ticker, o.order_id);
                                    let _ = ibkr_client.cancel_order(o.order_id, "").await;
                                }
                            }
                        }
                        println!(
                            "[reconcile-{}] Re-placing GTC bracket: SL=${:.2} TP=${:.2}",
                            ticker, pos.stop_loss, pos.take_profit
                        );
                        let legs = pos.bracket_legs(ticker);
                        match orders.place_gtc_bracket(&legs).await {
                            Ok(ids) => {
                                println!(
                                    "[reconcile-{}] Bracket placed: SL_oid={} TP_oid={} — verifying...",
                                    ticker, ids.stop_loss_id, ids.take_profit_id
                                );
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                let verified = match fetch_ibkr_orders(ibkr_client).await {
                                    Some(ords) => {
                                        let ticker_ords = ords.get(ticker.as_str());
                                        let sl_ok = ticker_ords.map(|o| o.iter().any(|x| x.order_id == ids.stop_loss_id)).unwrap_or(false);
                                        let tp_ok = ticker_ords.map(|o| o.iter().any(|x| x.order_id == ids.take_profit_id)).unwrap_or(false);
                                        sl_ok && tp_ok
                                    }
                                    None => {
                                        eprintln!("[reconcile-{}] Could not verify bracket — order fetch failed", ticker);
                                        true // optimistic: don't emergency-close if we can't verify
                                    }
                                };
                                if verified {
                                    println!("[reconcile-{}] Bracket verified OK", ticker);
                                    ts.bracket_ids = Some(ids);
                                    ts.save_snapshot();
                                } else {
                                    eprintln!(
                                        "[reconcile-{}] Bracket NOT verified — orders rejected by IBKR. EMERGENCY CLOSE",
                                        ticker
                                    );
                                    ts.force_close(&orders, "unprotected_no_bracket").await;
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[live-{}] Failed to re-place bracket: {:?} — EMERGENCY CLOSE",
                                    ticker, e
                                );
                                ts.force_close(&orders, "unprotected_no_bracket").await;
                            }
                        }
                    } else {
                        eprintln!(
                            "[live-{}] Position has no bracket and no saved SL/TP — EMERGENCY CLOSE",
                            ticker
                        );
                        ts.force_close(&orders, "unprotected_no_bracket").await;
                    }
                }
            }
        } else {
            eprintln!(
                "[reconcile] Skipping — IBKR data incomplete (positions={} orders={})",
                if ibkr_positions.is_some() { "ok" } else { "FAILED" },
                if ibkr_orders.is_some() { "ok" } else { "FAILED" },
            );
        }

        // ── Short-position safety net (long-only strategy) ───────────
        if let Some(ref positions) = ibkr_positions {
            for (sym, snap) in positions {
                if snap.shares < -0.001 {
                    let cover_qty = snap.shares.abs().ceil().trunc_u32();
                    eprintln!(
                        "[SAFETY] SHORT DETECTED: {} x{} — auto-covering (long-only strategy)",
                        sym, cover_qty
                    );
                    if let Some(ticker) = tickers.iter().find(|t| t.as_str() == sym) {
                        if let Err(e) = orders.place_market_buy(*ticker, cover_qty).await {
                            eprintln!("[SAFETY] CRITICAL: cover buy failed for {}: {:?}", sym, e);
                        }
                    } else {
                        eprintln!(
                            "[SAFETY] Short in {} but not a tracked ticker — cover manually!",
                            sym
                        );
                    }
                }
            }
        }

        // ── Fetch bars + check IBKR fills ────────────────────────────
        let mut ticker_bars: HashMap<Ticker, Vec<OhlcBar>> = HashMap::new();

        for &ticker in tickers {
            let ts = states.get_mut(&ticker).expect("ticker missing from states");
            let today_bars = orders.fetch_ticker_bars(ticker, ts).await;
            ts.check_bracket_fills(&ctx.fill_state, ibkr_client).await;
            ticker_bars.insert(ticker, today_bars);
        }

        // ── Process new bars + collect entry candidates ──────────────
        let mut entry_candidates: Vec<LiveEntryCandidate> = Vec::new();
        for ts in states.values_mut() {
            ts.had_new_data = false;
        }

        for &ticker in tickers {
            let today_bars = match ticker_bars.get(&ticker) {
                Some(b) => b,
                None => continue,
            };
            let ts = states.get_mut(&ticker).expect("ticker missing from states");

            if today_bars.is_empty() {
                update_health(&ctx.health_state, ticker.as_str(), ts.health_snapshot(&None, None, 0));
                continue;
            }

            let new_bars: Vec<&OhlcBar> = today_bars
                .iter()
                .filter(|b| b.timestamp.timestamp_millis() > ts.last_processed_ms)
                .collect();

            if let Some(last_bar) = new_bars.last() {
                ts.had_new_data = true;
                ts.last_fresh_data_ms = crate::types::now_ms();
                ts.spot_price = last_bar.close;
                log_debug!(
                    "[live-{}] IBKR {}m bar #{} | close=${:.2} | {} new",
                    ticker, interval, today_bars.len(), ts.spot_price, new_bars.len(),
                );
            }

            let now_entry_ms = crate::types::now_ms();
            let newest_bar_ms = today_bars
                .last()
                .map(|b| crate::types::datetime_millis_u64(&b.timestamp))
                .unwrap_or(0);
            let gex_last_ms = { super::lock_or_recover(&ctx.live_gex).last_poll_ms };
            let data_fresh = now_entry_ms.saturating_sub(newest_bar_ms) < freshness_timeout_ms
                && now_entry_ms.saturating_sub(gex_last_ms) < freshness_timeout_ms;

            if !data_fresh && !new_bars.is_empty() {
                let bar_age = now_entry_ms.saturating_sub(newest_bar_ms) / 1000;
                let gex_age = now_entry_ms.saturating_sub(gex_last_ms) / 1000;
                eprintln!(
                    "[live-{}] Skipping entry decisions — stale data (bar {}s, GEX {}s)",
                    ticker, bar_age, gex_age
                );
            }

            for bar in &new_bars {
                ts.last_processed_ms = bar.timestamp.timestamp_millis();
                let current_indicators = ts.engine.update_bar(bar);
                let indicators = match &current_indicators {
                    Some(iv) => iv,
                    None => continue,
                };
                let gex = match get_live_gex_profile(&ctx.live_gex, ticker) {
                    Some(g) => g,
                    None => continue,
                };

                let pipe = GexPipelineBar { bar, gex: &gex, indicators, ticker, verbose: false, entry_when: data_fresh };
                let (_signal, trail, entry_data) = ts.run_gex_bar_pipeline(pipe);
                match trail {
                    WallTrailOutcome::Ratcheted { old_sl, new_sl } => {
                        let shares = ts.position.as_ref().map(|p| p.shares).unwrap_or(0);
                        if let Some(ref mut bi) = ts.bracket_ids {
                            match orders.replace_stop(
                                ticker, shares, bi.stop_loss_id, new_sl, old_sl, &ctx.fill_state,
                            ).await {
                                Ok((new_id, actual_sl)) => {
                                    log_debug!(
                                        "[live-{}] Wall trail: SL ${:.2} → ${:.2} (oid {}→{})",
                                        ticker, old_sl, actual_sl, bi.stop_loss_id, new_id
                                    );
                                    bi.stop_loss_id = new_id;
                                    if let Some(ref mut pos) = ts.position {
                                        pos.stop_loss = actual_sl;
                                    }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[live-{}] Failed to update IBKR SL order: {:?} — reverting local SL to ${:.2}",
                                        ticker, e, old_sl
                                    );
                                    if let Some(ref mut pos) = ts.position {
                                        pos.stop_loss = old_sl;
                                    }
                                }
                            }
                        } else {
                            eprintln!(
                                "[live-{}] Wall trail ratcheted SL ${:.2} → ${:.2} but no bracket IDs — reverting",
                                ticker, old_sl, new_sl
                            );
                            if let Some(ref mut pos) = ts.position {
                                pos.stop_loss = old_sl;
                            }
                        }
                    }
                    WallTrailOutcome::EarlyTp => {
                        eprintln!("[live-{}] Early Hurst TP triggered — closing position", ticker);
                        ts.force_close(&orders, "early_tp").await;
                    }
                    WallTrailOutcome::Unchanged => {}
                }
                if let Some(data) = entry_data {
                    entry_candidates.push(LiveEntryCandidate { ticker, data });
                }
            }
        }

        // ── Rank + execute entries ───────────────────────────────────
        if !entry_candidates.is_empty() {
            orders.rank_and_execute_entries(
                &mut entry_candidates, &mut states,
                max_pos, per_position_div,
            ).await;
        }

        // ── Update health + persist state ────────────────────────────
        {
            let mut hs = super::lock_or_recover(&ctx.health_state);
            hs.broker_connected = ibkr_client.is_connected();
        }
        let gex_error = { super::lock_or_recover(&ctx.live_gex).last_error.clone() };

        for &ticker in tickers {
            let ts = states.get(&ticker).expect("ticker missing from states");
            let today_bars = ticker_bars.get(&ticker);
            let last_bar = today_bars.and_then(|b| b.last());
            let bars_today = today_bars
                .map(|b| b.len().as_len_u32())
                .unwrap_or(0);
            update_health(&ctx.health_state, ticker.as_str(), ts.health_snapshot(&gex_error, last_bar, bars_today));
        }

        for &ticker in tickers {
            let ts = states.get(&ticker).expect("ticker missing from states");
            if ts.had_new_data {
                ts.save_snapshot();
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(poll_ms)).await;
    }

    // ── Shutdown ─────────────────────────────────────────────────────
    println!("[live] Saving state and disconnecting...");
    for &ticker in tickers {
        states.get(&ticker).expect("ticker missing from states").save_snapshot();
    }
    drop(ctx.ibkr_client);
    let _ = broker.disconnect().await;

    if let Some(reason) = exit_reason {
        eprintln!("[live] Exiting due to {} — container will restart", reason);
        std::process::exit(1);
    }
    println!("[live] Shutdown complete.");
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Ticker;
    use crate::strategy::engine::{rank_and_dedup, EntryAtrTsi, EntryCandidateData};
    use crate::types::Signal;

    fn candidate(ticker: Ticker, tsi: f64) -> LiveEntryCandidate {
        LiveEntryCandidate {
            ticker,
                data: EntryCandidateData {
                signal: Signal::LongVannaFlip,
                reason: String::new(),
                entry_price: 100.0,
                atr_tsi: EntryAtrTsi::new(1.0, tsi),
                adx: 20.0,
                net_gex: 0.0,
                gex_spot: 100.0,
                tp_cap_atr: 0.0,
            },
        }
    }

    #[test]
    fn dedup_keeps_best_per_ticker() {
        let mut candidates = vec![
            candidate(Ticker::AAPL, 30.0),
            candidate(Ticker::GOOG, 20.0),
            candidate(Ticker::AAPL, 10.0),
        ];
        rank_and_dedup(&mut candidates);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].ticker, Ticker::AAPL);
        assert!((candidates[0].data.tsi() - 30.0).abs() < 0.01);
    }

    #[test]
    fn ranking_by_tsi() {
        let mut candidates = vec![
            candidate(Ticker::AAPL, 20.0),
            candidate(Ticker::GOOG, 10.0),
            candidate(Ticker::MSFT, 30.0),
        ];
        rank_and_dedup(&mut candidates);
        assert_eq!(candidates[0].ticker, Ticker::MSFT);
        assert_eq!(candidates[1].ticker, Ticker::AAPL);
        assert_eq!(candidates[2].ticker, Ticker::GOOG);
    }
}
