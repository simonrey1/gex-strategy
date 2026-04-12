use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rayon::prelude::*;

use crate::config::{StrategyConfig, Ticker};
use crate::data::bin_cache;
use crate::data::hist::{build_month_gex, ensure_day_data, load_day_bars_and_gex, load_month_gex};
use crate::types::{F64Trunc, GexProfile, OhlcBar};

use super::day_data::DayData;
use super::splits::{apply_split_adjustment, split_ratio_for_date};
use super::types::TickerState;

pub fn print_header(
    rctx: &super::runner::RunnerCtx,
    config: &StrategyConfig,
    num_trading_days: usize,
    warmup_days: usize,
) {
    println!("\n[backtest] ===================================================");
    println!(
        "[backtest] Portfolio: {:?}  {} -> {}  (1m exec, {}m strategy)",
        rctx.tickers, rctx.start_date, rctx.end_date, rctx.interval
    );
    println!(
        "[backtest] Capital=${}  MaxPos={:.0}%  SL={:.1}×ATR  TP={:.1}×ATR  MaxOpen={}  ExecDelay={}bars(1m)",
        rctx.bc.starting_capital.trunc_u64(),
        config.max_position_pct * 100.0,
        config.bracket_sl_atr(),
        config.bracket_tp_atr(),
        config.max_open_positions,
        rctx.bc.execution_delay_bars,
    );
    println!(
        "[backtest] {} trading days + {} warmup days",
        num_trading_days, warmup_days,
    );
}

pub async fn connect_ibkr(needs_ibkr: bool, verbose: bool) -> Result<Option<Arc<ibapi::Client>>> {
    if !needs_ibkr {
        if verbose {
            println!("[backtest] All 1m bars cached — IBKR not needed");
        }
        return Ok(None);
    }

    if verbose {
        eprintln!("[backtest] IBKR connection needed — some 1m bars missing");
    }

    let host = crate::config::ibkr_host();
    let port = crate::config::ibkr_port();
    let client_id = crate::config::ibkr_client_id_backtest();
    let addr = format!("{}:{}", host, port);

    let max_retries = 3u32;
    for attempt in 0..max_retries {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            ibapi::Client::connect(&addr, client_id),
        )
        .await
        {
            Ok(Ok(c)) => {
                if verbose {
                    println!("[backtest] IBKR connected");
                }
                return Ok(Some(Arc::new(c)));
            }
            _ if attempt + 1 < max_retries => {
                let delay = 5 * 2u64.pow(attempt);
                eprintln!(
                    "[backtest] IBKR connection to {} failed — retry {}/{} in {}s",
                    addr, attempt + 1, max_retries, delay
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
            _ => {
                anyhow::bail!(
                    "[backtest] IBKR needed (uncached 1m bars exist) but connection to {} failed after {} attempts — aborting",
                    addr, max_retries
                );
            }
        }
    }
    anyhow::bail!("IBKR connection: retry loop exited without result");
}

/// Pass 1: download all data (bars1m, raw options, monthly GEX) for every
/// ticker/day. Can take hours on first run; subsequent runs are instant.
pub async fn download_all_data(
    tickers: &[Ticker],
    month_groups: &[Vec<String>],
    ibkr: &Option<Arc<ibapi::Client>>,
) -> Result<()> {
    for month_days in month_groups {
        if month_days.is_empty() { continue; }
        let month = &month_days[0][..7];

        for &ticker in tickers {
            let gex_months = bin_cache::cached_gex_months(ticker);
            let gex_cached = gex_months.contains(month);
            let bar_dates = bin_cache::cached_bar_dates(ticker);

            let mut missing_days = Vec::new();
            for day in month_days {
                if !bar_dates.contains(day.as_str()) {
                    missing_days.push(day.clone());
                } else if !gex_cached {
                    let wide_path = ticker.raw_wide_path("backtest", day);
                    if !wide_path.exists() {
                        missing_days.push(day.clone());
                    }
                }
            }

            if !missing_days.is_empty() {
                eprintln!("[download] {} {}: ensuring {} days of data…", ticker, month, missing_days.len());
                for day in &missing_days {
                    ensure_day_data(ticker, day, ibkr).await?;
                }
            }

            if !missing_days.is_empty() {
                crate::data::hist::delete_month_gex(ticker, &month_days[0]);
            }
            if !gex_cached || !missing_days.is_empty() {
                build_month_gex(ticker, month_days)?;
            }
        }
    }
    Ok(())
}

/// Load monthly GEX for all tickers into memory.
pub fn load_month_gex_all(
    tickers: &[Ticker],
    month_days: &[String],
) -> HashMap<Ticker, Arc<HashMap<i64, GexProfile>>> {
    if month_days.is_empty() { return HashMap::new(); }
    tickers.par_iter().map(|&ticker| {
        let gex = load_month_gex(ticker, &month_days[0])
            .unwrap_or_else(|| Arc::new(HashMap::new()));
        (ticker, gex)
    }).collect()
}

/// Per-ticker warmup data: bars with GEX + GEX map.
pub struct WarmupData {
    pub bars_with_gex: Vec<OhlcBar>,
    pub gex_map: HashMap<i64, GexProfile>,
}

/// Load warmup bars + GEX for signal state + IV baseline.
pub fn load_warmup_bars(
    tickers: &[Ticker],
    bars_and_gex_days: &[String],
    verbosity: u8,
) -> HashMap<Ticker, WarmupData> {
    let mut result: HashMap<Ticker, WarmupData> = HashMap::new();
    for &t in tickers {
        result.insert(t, WarmupData { bars_with_gex: Vec::new(), gex_map: HashMap::new() });
    }

    let gex_months = group_by_month(bars_and_gex_days);
    for month_days in &gex_months {
        let month_gex = load_month_gex_all(tickers, month_days);
        let empty_gex: Arc<HashMap<i64, GexProfile>> = Arc::new(HashMap::new());
        for day in month_days {
            let raw: Vec<_> = tickers.par_iter().map(|&ticker| {
                load_day_bars_and_gex(ticker, day).map(|d| (ticker, d))
            }).collect::<Result<Vec<_>>>().unwrap_or_default();

            let (day_start, day_end) = crate::data::hist::day_epoch_range(day);
            for (ticker, (_, mut bars_15m)) in raw {
                if let Ok(naive) = chrono::NaiveDate::parse_from_str(day, crate::types::DATE_FMT) {
                    let ratio = split_ratio_for_date(ticker.as_str(), naive);
                    if (ratio - 1.0).abs() > 0.01 && verbosity >= 2 {
                        eprintln!("[warmup-split] {} {}: adjusting x{}", ticker, day, ratio);
                    }
                    apply_split_adjustment(&mut bars_15m, ratio);
                }
                let mgex = month_gex.get(&ticker).unwrap_or(&empty_gex);
                let wd = result.get_mut(&ticker).expect("ticker");
                wd.bars_with_gex.extend(bars_15m);
                for (&ts, v) in mgex.iter().filter(|(&ts, _)| ts >= day_start && ts < day_end) {
                    wd.gex_map.insert(ts, v.clone());
                }
            }
        }
    }

    result
}

fn group_by_month(days: &[String]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for day in days {
        let month = &day[..7];
        if groups.last().is_none_or(|g| &g[0][..7] != month) {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("non-empty").push(day.clone());
    }
    groups
}

/// Load cached day data for a trading day (pure read, no network).
/// All trading days are post-warmup so GEX is always included.
pub fn load_day_data(
    rctx: &super::runner::RunnerCtx,
    day: &str,
    states: &mut HashMap<Ticker, TickerState>,
) -> Result<DayData> {
    let raw: Vec<_> = rctx.tickers.par_iter().map(|&ticker| {
        load_day_bars_and_gex(ticker, day).map(|d| (ticker, d))
    }).collect::<Result<Vec<_>>>()?;

    let mut day_bars_1m: HashMap<Ticker, Vec<OhlcBar>> = HashMap::new();
    let mut day_bars_15m: HashMap<Ticker, Vec<OhlcBar>> = HashMap::new();

    for (ticker, (mut bars_1m, mut bars_15m)) in raw {
        let ts = match states.get_mut(&ticker) {
            Some(ts) => ts,
            None => continue,
        };

        if let Ok(naive) = chrono::NaiveDate::parse_from_str(day, crate::types::DATE_FMT) {
            let ratio = split_ratio_for_date(ticker.as_str(), naive);
            ts.chart_split_ratio = ratio;
            if (ratio - 1.0).abs() > 0.01 && !ts.split_logged {
                if rctx.verbose_ge(2) {
                    eprintln!("[split] {} {}: adjusting bars x{} (known split table)", ticker, day, ratio);
                }
                ts.split_logged = true;
            }
            apply_split_adjustment(&mut bars_1m, ratio);
            apply_split_adjustment(&mut bars_15m, ratio);
        }

        day_bars_1m.insert(ticker, bars_1m);
        day_bars_15m.insert(ticker, bars_15m);
    }
    Ok((day_bars_1m, day_bars_15m))
}
