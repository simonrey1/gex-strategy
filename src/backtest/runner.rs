use anyhow::Result;
use std::collections::HashMap;

use crate::config::bar_interval::{self, Minutes};
use crate::config::{BacktestConfig, StrategyConfig, Ticker};
use crate::strategy::indicators::IndicatorValues;
use crate::data::hist::ts_key;

use super::chart::ScanEntryMarkerInputs;
use super::equity::EquityTracker;
use super::execution::ExecContext;
use super::iv_scan::IvScanTracker;
use super::metrics::EtFormat;
use super::positions::get_trading_days;
use super::results::PortfolioResult;
use super::setup::{connect_ibkr, download_all_data, load_day_data, load_month_gex_all, load_warmup_bars, print_header};
use super::types::*;

use crate::strategy::engine::{RankedCandidate, WallTrailOutcome};
use crate::strategy::ranked_candidate::rank_dedup_and_remaining_slots;
use crate::strategy::shared::GexPipelineBar;
use crate::types::{GexProfile, OhlcBar, ToF64};

/// Backtest-level constants shared across runner, setup, results, and execution.
pub struct RunnerCtx<'a> {
    pub tickers: &'a [Ticker],
    pub start_date: &'a str,
    pub end_date: &'a str,
    pub bc: &'a BacktestConfig,
    pub starting_capital: f64,
    pub verbosity: u8,
    pub save_json: bool,
    pub interval: Minutes,
    pub max_pos: usize,
    pub per_position_pct_div: f64,
}

impl<'a> RunnerCtx<'a> {
    #[inline]
    pub fn exec_context(
        &'a self,
        ticker: Ticker,
        config: &'a StrategyConfig,
        portfolio_equity: f64,
    ) -> ExecContext<'a> {
        ExecContext {
            ticker,
            bc: self.bc,
            config,
            portfolio_equity,
            max_pos: self.max_pos,
            per_position_pct_div: self.per_position_pct_div,
            starting_capital: self.starting_capital,
            verbosity: self.verbosity,
        }
    }

    #[inline]
    pub fn verbose_ge(&self, level: u8) -> bool {
        self.verbosity >= level
    }

    /// [`GexPipelineBar`] for strategy bars (`verbose` follows backtest verbosity).
    #[inline]
    pub fn gex_pipeline_bar<'b>(
        &'b self,
        bar: &'b OhlcBar,
        gex: &'b GexProfile,
        indicators: &'b IndicatorValues,
        ticker: Ticker,
        entry_when: bool,
    ) -> GexPipelineBar<'b> {
        GexPipelineBar { bar, gex, indicators, ticker, verbose: self.verbose_ge(3), entry_when }
    }
}

struct EntryCandidate {
    ticker: Ticker,
    pending: PendingEntry,
}

impl RankedCandidate for EntryCandidate {
    fn ticker(&self) -> Ticker { self.ticker }
    fn rank_score(&self) -> f64 { self.pending.regime().tsi }
}

pub async fn run_portfolio_backtest(
    tickers: &[Ticker],
    start_date: &str,
    end_date: &str,
    strategy_config: &StrategyConfig,
    backtest_config: &BacktestConfig,
    verbosity: u8,
    save_json: bool,
    enable_iv_scan: bool,
) -> Result<PortfolioResult> {
    let interval = crate::config::BAR_INTERVAL_MINUTES;
    let (max_pos, per_position_pct_div) = strategy_config.slot_params();
    let all_days = get_trading_days(start_date, end_date);
    let start = chrono::NaiveDate::parse_from_str(start_date, crate::types::DATE_FMT)
        .expect("invalid start_date");

    let bars_cal = strategy_config.warmup_gex_calendar_days();
    let first_trading = start + chrono::Duration::days(i64::from(bars_cal));
    let first_trading_str = first_trading.format(crate::types::DATE_FMT).to_string();
    let warmup_day_count = all_days.partition_point(|d| *d < first_trading_str);
    if warmup_day_count == 0 || warmup_day_count >= all_days.len() {
        anyhow::bail!(
            "Date range has {} trading days but warmup needs {} cal days. Extend the range.",
            all_days.len(), bars_cal,
        );
    }

    let split = strategy_config.warmup_day_split(first_trading);
    let bars_and_gex_days = &split.gex_days;
    let trading_days = &all_days[warmup_day_count..];

    let rctx = RunnerCtx {
        tickers,
        start_date: &trading_days[0],
        end_date,
        bc: backtest_config,
        starting_capital: backtest_config.starting_capital,
        verbosity, save_json, interval,
        max_pos, per_position_pct_div,
    };

    if verbosity >= 1 {
        print_header(&rctx, strategy_config, trading_days.len(), warmup_day_count);
    }

    // Group ALL days by month (for download pass)
    let mut all_month_groups: Vec<Vec<String>> = Vec::new();
    for day in &all_days {
        let month = &day[..7];
        if all_month_groups.last().is_none_or(|g| &g[0][..7] != month) {
            all_month_groups.push(Vec::new());
        }
        all_month_groups.last_mut().expect("non-empty").push(day.clone());
    }

    // ══════════════════════════════════════════════════════════════════════
    // Pass 1: download all data (bars + options + monthly GEX cache)
    // ══════════════════════════════════════════════════════════════════════
    {
        let needs_ibkr = rctx.tickers.iter().any(|t| {
            let bar_dates = crate::data::bin_cache::cached_bar_dates(*t);
            let missing: Vec<_> = all_days.iter()
                .filter(|day| !bar_dates.contains(day.as_str()))
                .collect();
            if !missing.is_empty() && rctx.verbose_ge(1) {
                eprintln!(
                    "[backtest] {} missing {} bar dates (first 5: {:?})",
                    t, missing.len(), &missing[..missing.len().min(5)],
                );
            }
            !missing.is_empty()
        });
        let ibkr = connect_ibkr(needs_ibkr, rctx.verbose_ge(1)).await?;
        download_all_data(rctx.tickers, &all_month_groups, &ibkr).await?;
    }

    // ══════════════════════════════════════════════════════════════════════
    // Pass 2a: warmup — replay historical bars through shared pipeline
    // ══════════════════════════════════════════════════════════════════════
    let mut states: HashMap<Ticker, TickerState> = HashMap::new();
    for &t in rctx.tickers {
        let mut ts = TickerState::new(strategy_config);
        ts.save_chart = rctx.save_json;
        states.insert(t, ts);
    }

    let mut skipped_tickers: Vec<Ticker> = Vec::new();
    {
        let warmup_data = load_warmup_bars(rctx.tickers, bars_and_gex_days, verbosity);
        for &ticker in rctx.tickers {
            let ts = states.get_mut(&ticker).expect("ticker missing from states");
            if let Some(wd) = warmup_data.get(&ticker) {
                match ts.engine.warm_up(
                    &wd.bars_with_gex, &wd.gex_map,
                    &mut ts.wall_smoother, &mut ts.hurst, strategy_config, ticker,
                    verbosity >= 2,
                ) {
                    Ok(result) => {
                        if verbosity >= 2 {
                            println!(
                                "[warmup] {} replayed {} bars ({} with signal)",
                                ticker, result.bars_replayed, result.signal_bars,
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[warmup] {} SKIPPED: {}", ticker, e);
                        skipped_tickers.push(ticker);
                    }
                }
            } else {
                eprintln!("[warmup] {} SKIPPED: no cached data", ticker);
                skipped_tickers.push(ticker);
            }
        }
    }
    for &t in &skipped_tickers {
        states.remove(&t);
    }
    if states.is_empty() {
        anyhow::bail!("warmup: all tickers skipped — no data available");
    }
    let active_tickers: Vec<Ticker> = rctx.tickers.iter()
        .copied()
        .filter(|t| states.contains_key(t))
        .collect();

    // ══════════════════════════════════════════════════════════════════════
    // Pass 2b: trading (pure cache reads, no network)
    // ══════════════════════════════════════════════════════════════════════

    // Group trading days by month
    let mut month_groups: Vec<Vec<String>> = Vec::new();
    for day in trading_days {
        let month = &day[..7];
        if month_groups.last().is_none_or(|g| &g[0][..7] != month) {
            month_groups.push(Vec::new());
        }
        month_groups.last_mut().expect("non-empty").push(day.clone());
    }

    // Record first trading bar per ticker for buy-and-hold calculation
    let mut first_bar_recorded: HashMap<Ticker, bool> = tickers.iter().map(|&t| (t, false)).collect();

    let mut cash = rctx.starting_capital;
    let mut equity_tracker = EquityTracker::new(rctx.starting_capital);

    let mut iv_scanner: Option<IvScanTracker> = if enable_iv_scan {
        Some(IvScanTracker::new(strategy_config.eff_iv_lookback_bars()))
    } else {
        None
    };

    let bucket_secs = bar_interval::bucket_secs_i64(rctx.interval);
    let mut total_ticks: u64 = 0;
    let mut capital_utilization_sum: f64 = 0.0;

    for month_days in &month_groups {
        let month_gex = load_month_gex_all(&active_tickers, month_days);

    for day in month_days {
        let (day_bars_1m, day_bars_15m) = load_day_data(
            &rctx, day, &mut states,
        )?;

        for ts in states.values_mut() {
            ts.reset_daily();
        }

        let mut next_15m: HashMap<Ticker, usize> = tickers.iter().map(|&t| (t, 0)).collect();

        let max_1m_count = day_bars_1m.values().map(|b| b.len()).max().unwrap_or(0);
        if max_1m_count == 0 {
            continue;
        }

        let boundary_flags: Vec<bool> = {
            let ref_bars = active_tickers.iter()
                .filter_map(|t| day_bars_1m.get(t))
                .next()
                .expect("at least one ticker has bars");
            (0..ref_bars.len()).map(|i| {
                if i + 1 >= ref_bars.len() { return true; }
                let t0 = ref_bars[i].timestamp.timestamp();
                let t1 = ref_bars[i + 1].timestamp.timestamp();
                (t0 - t0 % bucket_secs) != (t1 - t1 % bucket_secs)
            }).collect()
        };

        for bar_1m_idx in 0..max_1m_count {
            let current_portfolio_equity = EquityTracker::portfolio_total(cash, &states);
            let mut open_positions = states.values()
                .filter(|s| s.position.is_some())
                .count();
            // ── 1-min tick: execution fills + SL/TP ──────────────────────
            for &ticker in &active_tickers {
                let bars_1m = match day_bars_1m.get(&ticker) {
                    Some(b) => b,
                    None => continue,
                };
                let bar = match bars_1m.get(bar_1m_idx) {
                    Some(b) => b,
                    None => continue,
                };
                let gex_map = month_gex.get(&ticker).map(|a| a.as_ref());

                let ts = states.get_mut(&ticker).expect("ticker missing from states");
                ts.last_bar = Some(*bar);
                ts.tick_pending_timers();

                let ctx = rctx.exec_context(ticker, strategy_config, current_portfolio_equity);

                if let Some(consumed) = ts.fill_pending_entry(bar, cash, open_positions, &ctx) {
                    cash -= consumed;
                    open_positions += 1;
                }
                if let Some(returned) = ts.check_sltp_1m(bar, gex_map, &ctx) {
                    cash += returned;
                    open_positions = open_positions.saturating_sub(1);
                }
            }

            // ══════════════════════════════════════════════════════════════
            // Strategy bar tick: indicators + signals (at bucket boundary)
            // ══════════════════════════════════════════════════════════════
            let is_boundary = boundary_flags.get(bar_1m_idx).copied().unwrap_or(true);
            let mut entry_candidates: Vec<EntryCandidate> = Vec::new();

            if is_boundary {
            for &ticker in &active_tickers {
                let bars_15m = match day_bars_15m.get(&ticker) {
                    Some(b) => b,
                    None => continue,
                };
                let idx_15m = *next_15m.get(&ticker).unwrap_or(&0);
                let bar_15m = match bars_15m.get(idx_15m) {
                    Some(b) => b,
                    None => continue,
                };
                *next_15m.entry(ticker).or_insert(0) += 1;

                let gex_map = month_gex.get(&ticker).map(|a| a.as_ref());

                let ts = states.get_mut(&ticker).expect("ticker missing from states");
                let current_indicators = ts.engine.update_bar(bar_15m);
                let bar_time_sec = bar_15m.timestamp.timestamp();

                if ts.save_chart { ts.push_bar(bar_15m, bar_time_sec); }

                let gex = match gex_map.and_then(|m| m.get(&ts_key(&bar_15m.timestamp))) {
                    Some(g) => g,
                    None => continue,
                };
                let indicators = match &current_indicators {
                    Some(iv) => iv,
                    None => continue,
                };
                ts.gex_bars += 1;

                // Record first trading bar for buy-and-hold (once per ticker)
                if !first_bar_recorded[&ticker] {
                    ts.first_trading_bar = Some((bar_time_sec, bar_15m.close));
                    *first_bar_recorded.get_mut(&ticker).unwrap() = true;
                }

                let pipe = rctx.gex_pipeline_bar(
                    bar_15m,
                    gex,
                    indicators,
                    ticker,
                    ts.pending_entry.is_none(),
                );
                let (signal, trail, candidate_data) =
                    ts.run_gex_bar_pipeline(pipe, strategy_config);
                match trail {
                    WallTrailOutcome::Ratcheted { old_sl, new_sl } => {
                        if rctx.verbose_ge(2) {
                            eprintln!(
                                "  [trail] {} {} SL ${:.2} → ${:.2}",
                                ticker, EtFormat::utc(&bar_15m.timestamp), old_sl, new_sl,
                            );
                        }
                    }
                    WallTrailOutcome::EarlyTp => {
                        let gex_map_ref = month_gex.get(&ticker).map(|a| a.as_ref());
                        let ctx = rctx.exec_context(ticker, strategy_config, current_portfolio_equity);
                        if let Some(returned) = ts.close_at_market(bar_15m, "early_tp", gex_map_ref, &ctx) {
                            cash += returned;
                            open_positions = open_positions.saturating_sub(1);
                            if rctx.verbose_ge(2) {
                                eprintln!(
                                    "  [early_tp] {} {} exit at ${:.2}",
                                    ticker, EtFormat::utc(&bar_15m.timestamp), bar_15m.close,
                                );
                            }
                        }
                    }
                    WallTrailOutcome::Unchanged => {}
                }
                let spw = ts.engine.signal_state.smoothed_put_wall();
                if ts.save_chart {
                    ts.collect_wall_chart_data(gex, bar_time_sec);
                    ts.collect_iv_markers(bar_time_sec);
                    ts.collect_iv_ema_chart(bar_time_sec, bar_15m.is_eod());
                    ts.close_spike_window_if_expired(bar_time_sec);
                }

                let spike_tooltip = ts.save_chart
                    && ts.engine.signal_state.has_active_spike()
                    && signal.signal.is_flat();
                if spike_tooltip || iv_scanner.is_some() {
                    let bctx = ts.engine.bar_ctx(bar_15m, gex, indicators, strategy_config, ticker);
                    if spike_tooltip {
                        use crate::strategy::entries::wall_bounce::vf_ctx;
                        let reasons = match vf_ctx(&bctx) {
                            Err(r) => r,
                            Ok(vf) => vf.evaluate(true).err().unwrap_or_default(),
                        };
                        let lines = super::chart::SpikeBarDiag::compute(&bctx).format_lines(&reasons);
                        ts.chart_data.spike_tooltips.push(super::state::BarTooltip { time: bar_time_sec, lines });
                    }
                    if let Some(ref mut scanner) = iv_scanner {
                        scanner.update(ticker, spw, &bctx);
                        scanner.detect_and_open(ticker, &bctx);
                    }
                }

                if rctx.verbose_ge(3) && signal.signal.is_flat() && ts.position.is_none() {
                    let mode_pw = ts.engine.signal_state.smoothed_put_wall();
                    let mode_cw = ts.engine.signal_state.smoothed_call_wall();
                    let spread_pct = if mode_cw > mode_pw && gex.spot > 0.0 {
                        (mode_cw - mode_pw) / gex.spot * 100.0
                    } else { 0.0 };
                    eprintln!(
                        "  [flat] {} {} c={:.2} pw={:.0} cw={:.0} sp={:.1}% atr={:.2} adx={:.1} tsi={:.1} | {}",
                        ticker, EtFormat::utc(&bar_15m.timestamp),
                        bar_15m.close, mode_pw, mode_cw, spread_pct,
                        indicators.atr, indicators.adx, indicators.tsi,
                        signal.reason,
                    );
                }

                if let Some(candidate_data) = candidate_data {
                    if rctx.verbose_ge(3) {
                        eprintln!(
                            "  [sig] {} {} c={:.2} atr={:.2} adx={:.1} tsi={:.1} | {}",
                            ticker, EtFormat::utc(&bar_15m.timestamp),
                            bar_15m.close, indicators.atr, indicators.adx,
                            indicators.tsi,
                            signal.reason,
                        );
                    }
                    entry_candidates.push(EntryCandidate {
                        ticker,
                        pending: PendingEntry::from_candidate(
                            candidate_data, gex, bar_15m, rctx.bc,
                            indicators.atr_regime_ratio,
                            ts.engine.signal_state.iv_spike_bar,
                        ),
                    });
                }
            }

            // ── Rank entry candidates and schedule the best ones ──────────
            let open_or_pending = states.values().filter(|s| s.slot_held()).count();
            let slots = rank_dedup_and_remaining_slots(
                &mut entry_candidates,
                rctx.max_pos,
                open_or_pending,
            );
            for (i, c) in entry_candidates.into_iter().enumerate() {
                if let Some(ts) = states.get_mut(&c.ticker) {
                    if i < slots {
                        ts.engine.commit_entry(
                            c.pending.prepare.signal,
                            c.pending.diag.signal_bar_close,
                        );
                        ts.pending_entry = Some(c.pending);
                    }
                }
            }
            } // is_boundary

            total_ticks += 1;
            let equity = EquityTracker::portfolio_total(cash, &states);
            let bar_time = tickers.iter()
                .filter_map(|t| day_bars_1m.get(t).and_then(|b| b.get(bar_1m_idx)))
                .next()
                .map(|b| b.timestamp.timestamp())
                .unwrap_or(0);
            if equity > 0.0 {
                let invested: f64 = states.values().map(|ts| ts.mark_to_market()).sum();
                capital_utilization_sum += invested / equity;
            }
            equity_tracker.update(equity, bar_time);
        }
    }
    } // end month_groups

    if let Some(scanner) = iv_scanner {
        let results = scanner.finalize(rctx.verbose_ge(1));
        for r in results {
            if let Some(ts) = states.get_mut(&r.ticker) {
                ts.chart_data.markers.push(
                    super::state::Marker::scan_entry(ScanEntryMarkerInputs {
                        entry_sec: r.entry_time_sec,
                        exit_sec: r.exit_time_sec,
                        pct: r.profit_pct,
                        max_runup_atr: r.max_runup_atr,
                        exit_time: &r.exit_time,
                        bucket: r.bucket,
                    }),
                );
                if r.bucket == super::iv_scan::ScanBucket::Best {
                    ts.chart_data.markers.push(
                        super::state::Marker::scan_exit(r.exit_time_sec, r.profit_pct),
                    );
                }
                ts.iv_scan_results.push(r);
            }
        }
    }

    for ts in states.values_mut() {
        if let Some(w) = ts.chart_data.spike_windows.last_mut() {
            if w.end == 0 {
                w.end = ts.last_bar.map(|b| b.timestamp.timestamp()).unwrap_or(w.start);
            }
        }
    }

    rctx.finalize_positions(&mut states)?;

    let avg_capital_util_pct = if total_ticks > 0 {
        capital_utilization_sum / total_ticks.to_f64()
    } else {
        0.0
    };

    rctx.collect_results(
        &mut states,
        equity_tracker.curve, equity_tracker.max_drawdown, equity_tracker.max_drawdown_pct,
        avg_capital_util_pct,
    )
}
