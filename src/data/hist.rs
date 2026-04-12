use anyhow::Result;
use chrono::NaiveDate;
use std::collections::HashMap;
use std::sync::Arc;

use crate::broker::ibkr::fetch_ibkr_bars_for_date;
use crate::config::bar_interval::{self, Minutes};
use crate::config::Ticker;

use super::bin_cache;
use super::mem_cache;

pub const VERSION_GEX: u32 = 5;
pub const VERSION_RAW: u32 = 5;
use crate::types::{GexProfile, OhlcBar};

/// Per-ticker day data: (1-min bars, strategy-interval bars).
pub type DayBarsAndGex = (Vec<OhlcBar>, Vec<OhlcBar>);

use super::cache::{read_raw_gz, write_raw_gz};
use super::gex_builder::build_gex_from_wide;
use super::thetadata_hist::{
    cached_bars_to_ohlc, cached_gex_to_map, fetch_options_day_wide,
    gex_map_to_cached_entries, ohlc_to_cached, CachedBar, CachedGexEntry, CachedRawTheta,
};

pub use super::thetadata_hist::ts_key;

// ─── Bars: bulk load from binary on first access ────────────────────────────

static BARS_LOADED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<Ticker>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

fn load_bars_1m_cached(ticker: Ticker, date: &str) -> Option<Arc<Vec<OhlcBar>>> {
    if let Some(bars) = mem_cache::get_bars(ticker, date) {
        return Some(bars);
    }

    {
        let loaded = BARS_LOADED.lock().unwrap();
        if loaded.contains(&ticker) {
            return None;
        }
    }

    if let Some(all_cached) = bin_cache::read_all_bars(ticker) {
        bulk_populate_bars(ticker, all_cached);
    }
    BARS_LOADED.lock().unwrap().insert(ticker);
    mem_cache::get_bars(ticker, date)
}

fn bulk_populate_bars(ticker: Ticker, all_cached: Vec<CachedBar>) {
    let mut by_date: HashMap<String, Vec<OhlcBar>> = HashMap::new();
    for bar in cached_bars_to_ohlc(all_cached) {
        let date = bar.timestamp.format(crate::types::DATE_FMT).to_string();
        by_date.entry(date).or_default().push(bar);
    }
    let entries: Vec<_> = by_date
        .into_iter()
        .map(|(date, bars)| (ticker, date, bars))
        .collect();
    mem_cache::put_bars_bulk(entries);
}

// ─── GEX: bulk load from binary on first access ────────────────────────────

static GEX_LOADED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<Ticker>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

fn ensure_gex_loaded(ticker: Ticker) -> bool {
    {
        let loaded = GEX_LOADED.lock().unwrap();
        if loaded.contains(&ticker) {
            return false;
        }
    }
    let ok = if let Some(all_entries) = bin_cache::read_all_gex(ticker) {
        bulk_populate_gex(ticker, all_entries);
        true
    } else {
        false
    };
    GEX_LOADED.lock().unwrap().insert(ticker);
    ok
}

fn bulk_populate_gex(ticker: Ticker, all_entries: Vec<CachedGexEntry>) {
    let full_map = cached_gex_to_map(all_entries);
    let mut by_month: HashMap<String, HashMap<i64, GexProfile>> = HashMap::new();
    for (ts, profile) in full_map {
        let dt = chrono::DateTime::from_timestamp(ts, 0).expect("valid epoch in gex cache");
        let month = dt.format("%Y-%m").to_string();
        by_month.entry(month).or_default().insert(ts, profile);
    }
    let entries: Vec<_> = by_month
        .into_iter()
        .map(|(month, map)| (ticker, month, map))
        .collect();
    mem_cache::put_gex_bulk(entries);
}

// ─── Parallel preload (called once at startup) ──────────────────────────────

/// Read all binaries and convert into mem_cache in parallel across tickers.
/// After this, all `load_bars_1m_cached` and `load_month_gex` calls are instant.
pub fn preload_all(tickers: &[crate::config::Ticker]) {
    use rayon::prelude::*;
    tickers.par_iter().for_each(|&ticker| {
        if let Some(all_cached) = bin_cache::read_all_bars(ticker) {
            bulk_populate_bars(ticker, all_cached);
        }
        BARS_LOADED.lock().unwrap().insert(ticker);

        if let Some(all_entries) = bin_cache::read_all_gex(ticker) {
            bulk_populate_gex(ticker, all_entries);
        }
        GEX_LOADED.lock().unwrap().insert(ticker);
    });
}

// ─── Resample ───────────────────────────────────────────────────────────────

/// Resample 1-minute OhlcBars into N-minute bars.
/// The aggregated bar's timestamp is the FIRST 1-min bar in each bucket
/// (matching IBKR convention: bar timestamp = interval start).
pub fn resample_bars(bars: &[OhlcBar], interval_minutes: Minutes) -> Vec<OhlcBar> {
    if interval_minutes <= 1 || bars.is_empty() {
        return bars.to_vec();
    }
    let bucket_secs = bar_interval::bucket_secs_i64(interval_minutes);
    let im = bar_interval::minutes_stride_usize(interval_minutes);
    let mut out = Vec::with_capacity(bars.len() / im + 1);
    let mut i = 0;
    while i < bars.len() {
        let ts0 = bars[i].timestamp.timestamp();
        let bucket_start = ts0 - (ts0 % bucket_secs);
        let bucket_end = bucket_start + bucket_secs;
        let mut agg = OhlcBar {
            timestamp: bars[i].timestamp,
            open: bars[i].open,
            high: bars[i].high,
            low: bars[i].low,
            close: bars[i].close,
            volume: bars[i].volume,
        };
        i += 1;
        while i < bars.len() && bars[i].timestamp.timestamp() < bucket_end {
            if bars[i].high > agg.high { agg.high = bars[i].high; }
            if bars[i].low < agg.low { agg.low = bars[i].low; }
            agg.close = bars[i].close;
            agg.volume += bars[i].volume;
            i += 1;
        }
        out.push(agg);
    }
    out
}

// ─── Wide raw data (full-chain 15-min greeks) ────────────────────────────────

fn is_theta_bad_date(ticker: Ticker, date: NaiveDate) -> bool {
    let s = date.format(crate::types::DATE_FMT).to_string();
    crate::backtest::calendar::is_ticker_bad_date(ticker, &s)
}

pub async fn load_or_fetch_wide_raw(
    ticker: Ticker,
    date: NaiveDate,
    cache_path: &std::path::Path,
) -> Option<CachedRawTheta> {
    if let Some(raw) = read_raw_gz::<CachedRawTheta>(cache_path) {
        if raw.oi.is_empty() && raw.second_order.is_empty() && raw.all_greeks.is_empty() {
            return None;
        }
        return Some(raw);
    }
    if is_theta_bad_date(ticker, date) {
        eprintln!("[wide_raw] {} {}: skipped (known ThetaData bad date)", ticker, date);
        let empty = CachedRawTheta { oi: vec![], greeks: vec![], second_order: vec![], all_greeks: vec![] };
        let _ = write_raw_gz(cache_path, &empty);
        return None;
    }
    match fetch_options_day_wide(ticker, date).await {
        Ok(raw) => {
            if let Err(e) = write_raw_gz(cache_path, &raw) {
                eprintln!("[wide_raw] {} {}: cache write failed: {}", ticker, date, e);
            }
            if raw.oi.is_empty() && raw.second_order.is_empty() && raw.all_greeks.is_empty() {
                return None;
            }
            Some(raw)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if msg.contains("472") || msg.contains("No data found") {
                let empty = CachedRawTheta { oi: vec![], greeks: vec![], second_order: vec![], all_greeks: vec![] };
                if let Err(e) = write_raw_gz(cache_path, &empty) {
                    eprintln!("[cache] Failed to write empty sentinel for {} {}: {}", ticker, date, e);
                }
            } else {
                eprintln!("[wide_raw] {} {}: fetch failed, writing empty sentinel: {}", ticker, date, msg.chars().take(120).collect::<String>());
                let empty = CachedRawTheta { oi: vec![], greeks: vec![], second_order: vec![], all_greeks: vec![] };
                let _ = write_raw_gz(cache_path, &empty);
            }
            None
        }
    }
}

// ─── IBKR bar helpers ────────────────────────────────────────────────────────

#[cfg(test)]
fn bars_are_synthesized(bars: &[CachedBar]) -> bool {
    !bars.is_empty()
        && bars
            .iter()
            .all(|b| b.open == b.high && b.high == b.low && b.low == b.close)
}

const IBKR_BAR_RETRIES: u32 = 3;
const IBKR_RETRY_DELAY_SECS: u64 = 5;

async fn try_ibkr_bars(
    ibkr: &Option<Arc<ibapi::Client>>,
    ticker: Ticker,
    date: &str,
) -> Result<Option<Vec<OhlcBar>>> {
    let client = match ibkr.as_ref() {
        Some(c) => c,
        None => return Ok(None),
    };
    let naive_date = match chrono::NaiveDate::parse_from_str(date, crate::types::DATE_FMT) {
        Ok(d) => d,
        Err(_) => return Ok(None),
    };
    for attempt in 0..IBKR_BAR_RETRIES {
        match fetch_ibkr_bars_for_date(client, ticker, naive_date).await {
            Ok(bars) if !bars.is_empty() => {
                let first_date = bars[0].timestamp.format(crate::types::DATE_FMT).to_string();
                if first_date != date {
                    eprintln!("[ibkr] {} {}: bars belong to {} (holiday?) — skipping", ticker, date, first_date);
                    return Ok(None);
                }
                println!(
                    "[ibkr] {} {}: fetched {} 1m bars",
                    ticker, date, bars.len()
                );
                return Ok(Some(bars));
            }
            Ok(_) => return Ok(None),
            Err(e) => {
                let msg = format!("{e:#}");
                if attempt + 1 < IBKR_BAR_RETRIES {
                    let delay = IBKR_RETRY_DELAY_SECS * 2u64.pow(attempt);
                    eprintln!(
                        "[ibkr] {} {}: retry {}/{} in {}s: {}",
                        ticker, date, attempt + 1, IBKR_BAR_RETRIES, delay,
                        msg.chars().take(120).collect::<String>(),
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                } else {
                    anyhow::bail!(
                        "[ibkr] {} {}: bar fetch failed after {} attempts: {}",
                        ticker, date, IBKR_BAR_RETRIES, msg
                    );
                }
            }
        }
    }
    Ok(None)
}

// ─── Pass 1: data download ───────────────────────────────────────────────────

/// Ensure bars1m and raw options are cached for one ticker/day.
/// Fetches from IBKR (bars) and ThetaData (options) if missing.
pub async fn ensure_day_data(
    ticker: Ticker,
    date: &str,
    ibkr: &Option<Arc<ibapi::Client>>,
) -> Result<()> {
    let bar_dates = bin_cache::cached_bar_dates(ticker);
    let has_bars = bar_dates.contains(date);

    if !has_bars {
        let day_path = super::cache::day_dir_for("backtest", ticker.as_str(), date)
            .join("processed_bars1m.json");
        if let Some(cached) = super::cache::read_processed::<Vec<CachedBar>>(&day_path) {
            let ohlc = cached_bars_to_ohlc(cached.clone());
            bin_cache::append_day_bars(ticker, date, &cached)?;
            mem_cache::put_bars(ticker, date, ohlc);
        } else {
            match try_ibkr_bars(ibkr, ticker, date).await? {
                Some(real_bars) => {
                    let cached = ohlc_to_cached(&real_bars);
                    let day_path = super::cache::day_dir_for("backtest", ticker.as_str(), date)
                        .join("processed_bars1m.json");
                    super::cache::write_processed(&day_path, &cached).ok();
                    bin_cache::append_day_bars(ticker, date, &cached)?;
                    mem_cache::put_bars(ticker, date, real_bars);
                }
                None => {
                    bin_cache::append_day_bars(ticker, date, &[])?;
                }
            }
        }
    }

    let wide_raw_path = ticker.raw_wide_path("backtest", date);
    let naive_date = NaiveDate::parse_from_str(date, crate::types::DATE_FMT)
        .expect("invalid date in download_all_data");
    load_or_fetch_wide_raw(ticker, naive_date, &wide_raw_path).await;
    Ok(())
}

// ─── Monthly GEX builder (cache-only, no fetching) ──────────────────────────

/// Build monthly GEX cache for one ticker from already-cached daily data.
/// Appends to the per-ticker binary. No-op if the month's data already exists.
pub fn build_month_gex(
    ticker: Ticker,
    days: &[String],
) -> Result<()> {
    if days.is_empty() { return Ok(()); }

    let month = &days[0][..7];
    let cached_months = bin_cache::cached_gex_months(ticker);
    if cached_months.contains(month) { return Ok(()); }

    let interval = crate::config::BAR_INTERVAL_MINUTES;

    // Pre-load bars into mem_cache (bulk load on first access)
    load_bars_1m_cached(ticker, &days[0]);

    use rayon::prelude::*;
    let day_results: Vec<(HashMap<i64, GexProfile>, Vec<CachedGexEntry>)> = days
        .par_iter()
        .filter_map(|day| {
            let bars_1m = mem_cache::get_bars(ticker, day)?;
            if bars_1m.is_empty() { return None; }
            let bars_15m = resample_bars(&bars_1m, interval);

            let wide_raw_path = ticker.raw_wide_path("backtest", day);
            let wide_raw = match read_raw_gz::<CachedRawTheta>(&wide_raw_path) {
                Some(raw) if !raw.oi.is_empty() || !raw.all_greeks.is_empty() => raw,
                _ => return None,
            };

            match build_gex_from_wide(ticker, day, &wide_raw, &bars_15m) {
                Ok(gex_map) => {
                    let entries = gex_map_to_cached_entries(&gex_map);
                    Some((gex_map, entries))
                }
                Err(e) => {
                    eprintln!("[gex_build] {} {}: build failed: {}", ticker, day, e);
                    None
                }
            }
        })
        .collect();

    let mut month_gex: HashMap<i64, GexProfile> = HashMap::new();
    let mut all_entries: Vec<CachedGexEntry> = Vec::new();
    for (gex_map, entries) in day_results {
        month_gex.extend(gex_map);
        all_entries.extend(entries);
    }
    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    bin_cache::append_month_gex(ticker, month, &all_entries)?;
    mem_cache::put_gex(ticker, month, month_gex);
    Ok(())
}

pub fn delete_month_gex(ticker: Ticker, any_date_in_month: &str) {
    let month = &any_date_in_month[..7];
    if let Err(e) = bin_cache::remove_month_gex(ticker, month) {
        eprintln!("[gex_build] {} delete month {}: {}", ticker, month, e);
    }
    mem_cache::evict_gex(ticker, month);
    GEX_LOADED.lock().unwrap().remove(&ticker);
}

// ─── Public entry point ──────────────────────────────────────────────────────

/// Load 1-min bars from cache, resample to 15m, extract day's GEX from monthly map.
/// Pure cache read — all data must already be downloaded (Pass 1).
pub fn load_day_bars_and_gex(
    ticker: Ticker,
    date: &str,
) -> Result<DayBarsAndGex> {
    let bars_1m_arc = match load_bars_1m_cached(ticker, date) {
        Some(b) => b,
        None => return Ok((vec![], vec![])),
    };
    if bars_1m_arc.is_empty() {
        return Ok((vec![], vec![]));
    }
    let first_bar_date = &bars_1m_arc[0].timestamp.format(crate::types::DATE_FMT).to_string();
    if first_bar_date != date {
        return Ok((vec![], vec![]));
    }

    let bars_15m = match mem_cache::get_bars_15m(ticker, date) {
        Some(cached) => (*cached).clone(),
        None => {
            let resampled = resample_bars(&bars_1m_arc, crate::config::BAR_INTERVAL_MINUTES);
            mem_cache::put_bars_15m(ticker, date, resampled.clone());
            resampled
        }
    };

    Ok(((*bars_1m_arc).clone(), bars_15m))
}

/// Epoch-second range [start, end) for a "YYYY-MM-DD" date string.
pub fn day_epoch_range(date: &str) -> (i64, i64) {
    let naive = NaiveDate::parse_from_str(date, crate::types::DATE_FMT).expect("valid date");
    let start = naive.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    (start, start + 86400)
}

/// Load monthly GEX cache. On first call for a ticker, loads the full binary
/// and populates all months into mem_cache.
pub fn load_month_gex(
    ticker: Ticker,
    any_date_in_month: &str,
) -> Option<Arc<HashMap<i64, GexProfile>>> {
    let month = &any_date_in_month[..7];

    if let Some(map) = mem_cache::get_gex(ticker, month) {
        return Some(map);
    }

    ensure_gex_loaded(ticker);
    mem_cache::get_gex(ticker, month)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use crate::config::BAR_INTERVAL_MINUTES;

    fn bar(minutes_offset: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> OhlcBar {
        OhlcBar {
            timestamp: chrono::Utc.with_ymd_and_hms(2025, 1, 10, 14, 30, 0).unwrap()
                + chrono::Duration::minutes(minutes_offset),
            open: o, high: h, low: l, close: c, volume: v,
        }
    }

    #[test]
    fn resample_passthrough_at_1m() {
        let bars = vec![bar(0, 100.0, 101.0, 99.0, 100.5, 100.0)];
        let r = resample_bars(&bars, 1);
        assert_eq!(r.len(), 1);
        assert!((r[0].open - 100.0).abs() < 0.01);
    }

    #[test]
    fn resample_empty() {
        let r = resample_bars(&[], BAR_INTERVAL_MINUTES);
        assert!(r.is_empty());
    }

    #[test]
    fn resample_aggregates_ohlcv() {
        let bars = vec![
            bar(0, 100.0, 105.0, 99.0, 102.0, 100.0),
            bar(1, 102.0, 106.0, 101.0, 104.0, 200.0),
            bar(2, 104.0, 104.5, 100.0, 103.0, 150.0),
        ];
        let r = resample_bars(&bars, BAR_INTERVAL_MINUTES);
        assert_eq!(r.len(), 1);
        assert!((r[0].open - 100.0).abs() < 0.01);
        assert!((r[0].high - 106.0).abs() < 0.01);
        assert!((r[0].low - 99.0).abs() < 0.01);
        assert!((r[0].close - 103.0).abs() < 0.01);
        assert!((r[0].volume - 450.0).abs() < 0.01);
    }

    #[test]
    fn resample_multiple_buckets() {
        let step = i64::from(BAR_INTERVAL_MINUTES);
        let bars = vec![
            bar(0, 100.0, 101.0, 99.0, 100.0, 10.0),
            bar(step, 100.0, 102.0, 98.0, 101.0, 20.0),
        ];
        let r = resample_bars(&bars, BAR_INTERVAL_MINUTES);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn bars_are_synthesized_true() {
        let bars = vec![
            CachedBar { timestamp: 1000, open: 50.0, high: 50.0, low: 50.0, close: 50.0, volume: 10.0 },
        ];
        assert!(bars_are_synthesized(&bars));
    }

    #[test]
    fn bars_are_synthesized_false() {
        let bars = vec![
            CachedBar { timestamp: 1000, open: 50.0, high: 51.0, low: 49.0, close: 50.0, volume: 10.0 },
        ];
        assert!(!bars_are_synthesized(&bars));
    }

    #[test]
    fn bars_are_synthesized_empty() {
        assert!(!bars_are_synthesized(&[]));
    }
}
