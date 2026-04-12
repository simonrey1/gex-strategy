use anyhow::Result;
use chrono::Datelike;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::config::Ticker;
use crate::data::hist::VERSION_GEX;

use super::cache::unified_dir;
use super::thetadata_hist::{CachedBar, CachedGexEntry, CachedRawTheta};

// ─── In-process caches (avoid re-reading full files) ─────────────────────

static BAR_DATES_CACHE: std::sync::LazyLock<Mutex<HashMap<Ticker, HashSet<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

static GEX_MONTHS_CACHE: std::sync::LazyLock<Mutex<HashMap<Ticker, HashSet<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// ─── Paths ──────────────────────────────────────────────────────────────────

fn unified_ticker_dir(ticker: &str) -> PathBuf {
    let dir = unified_dir().join(ticker);
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn bars_parquet_path(ticker: &str) -> PathBuf {
    unified_ticker_dir(ticker).join("bars_1m.parquet")
}

pub fn gex_parquet_path(ticker: &str) -> PathBuf {
    gex_parquet_path_versioned(ticker, VERSION_GEX)
}

pub fn gex_parquet_path_versioned(ticker: &str, version: u32) -> PathBuf {
    unified_ticker_dir(ticker).join(format!("gex_15m_v{version}.parquet"))
}

// ─── Bars ───────────────────────────────────────────────────────────────────

pub fn read_all_bars(ticker: Ticker) -> Option<Vec<CachedBar>> {
    let path = bars_parquet_path(ticker.as_str());
    let bars = super::parquet_cache::read_bars(&path)?;
    let dates = super::parquet_cache::read_metadata_set(&path, "processed_dates");
    BAR_DATES_CACHE.lock().unwrap().insert(ticker, dates);
    Some(bars)
}

/// Record a day's bars. `date` is always marked processed (even if empty/holiday).
pub fn append_day_bars(ticker: Ticker, date: &str, day_bars: &[CachedBar]) -> Result<()> {
    let path = bars_parquet_path(ticker.as_str());
    let mut dates = super::parquet_cache::read_metadata_set(&path, "processed_dates");
    if dates.contains(date) {
        return Ok(());
    }
    dates.insert(date.to_string());
    let mut all = super::parquet_cache::read_bars(&path).unwrap_or_default();
    if !day_bars.is_empty() {
        all.extend_from_slice(day_bars);
        all.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    }
    super::parquet_cache::write_bars(&path, &all, &dates)?;
    BAR_DATES_CACHE.lock().unwrap().insert(ticker, dates);
    Ok(())
}

/// All processed dates (including holidays). Cached in-process after first read.
pub fn cached_bar_dates(ticker: Ticker) -> HashSet<String> {
    {
        let lock = BAR_DATES_CACHE.lock().unwrap();
        if let Some(dates) = lock.get(&ticker) {
            return dates.clone();
        }
    }
    let dates = super::parquet_cache::read_metadata_set(
        &bars_parquet_path(ticker.as_str()), "processed_dates",
    );
    BAR_DATES_CACHE.lock().unwrap().insert(ticker, dates.clone());
    dates
}

// ─── One-time startup migration ─────────────────────────────────────────────

/// Ensure unified cache files exist and are complete.
/// Called once at startup. No-op if cache files already exist.
/// Note: `hist::preload_all` should be called after this to populate mem_cache.
pub fn ensure_binary_cache(tickers: &[Ticker]) {
    use rayon::prelude::*;
    tickers.par_iter().for_each(|&ticker| {
        migrate_bars_if_needed(ticker);
        migrate_gex_if_needed(ticker);
    });
}

fn migrate_bars_if_needed(ticker: Ticker) {
    let td = super::cache::cache_dir().join(ticker.as_str());
    if !td.exists() {
        return;
    }

    let path = bars_parquet_path(ticker.as_str());
    let mut dates = super::parquet_cache::read_metadata_set(&path, "processed_dates");
    let mut bars = super::parquet_cache::read_bars(&path).unwrap_or_default();
    let before = dates.len();

    let month_dirs = match std::fs::read_dir(&td) {
        Ok(e) => e,
        Err(_) => return,
    };
    for month_entry in month_dirs.flatten() {
        let mname = month_entry.file_name();
        let mstr = mname.to_string_lossy();
        if mstr.len() != 7 || !mstr.starts_with("20") {
            continue;
        }
        let Ok(day_dirs) = std::fs::read_dir(month_entry.path()) else { continue };
        for day_entry in day_dirs.flatten() {
            let dname = day_entry.file_name();
            let dstr = dname.to_string_lossy();
            if dstr.len() != 10 || !dstr.starts_with(&*mstr) {
                continue;
            }
            if dates.contains(dstr.as_ref()) {
                continue;
            }
            dates.insert(dstr.to_string());
            let json_path = day_entry.path().join("processed_bars1m.json");
            if let Some(bars_c) = super::cache::read_processed::<Vec<CachedBar>>(&json_path) {
                bars.extend(bars_c);
            }
        }
    }

    if dates.len() == before {
        return;
    }

    bars.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let n_bars = bars.len();
    match super::parquet_cache::write_bars(&path, &bars, &dates) {
        Ok(()) => {
            eprintln!("[migrate] {} bars: {} dates, {} bars", ticker, dates.len(), n_bars);
            BAR_DATES_CACHE.lock().unwrap().insert(ticker, dates);
        }
        Err(e) => eprintln!("[migrate] {} bars: write failed: {}", ticker, e),
    }
}

fn migrate_gex_if_needed(ticker: Ticker) {
    let path = gex_parquet_path(ticker.as_str());
    if path.exists() {
        return;
    }

    let td = super::cache::cache_dir().join(ticker.as_str());
    if !td.exists() {
        return;
    }

    let bars = match super::parquet_cache::read_bars(&bars_parquet_path(ticker.as_str())) {
        Some(b) if !b.is_empty() => b,
        _ => return,
    };

    let all_ohlc = super::thetadata_hist::cached_bars_to_ohlc(bars);
    let interval = crate::config::BAR_INTERVAL_MINUTES;
    let mut bars_by_date: HashMap<String, Vec<crate::types::OhlcBar>> = HashMap::new();
    for bar in &all_ohlc {
        let date = bar.timestamp.format(crate::types::DATE_FMT).to_string();
        bars_by_date.entry(date).or_default().push(*bar);
    }

    let mut sorted_dates: Vec<String> = bars_by_date.keys().cloned().collect();
    sorted_dates.sort();

    use rayon::prelude::*;
    let results: Vec<(Vec<CachedGexEntry>, bool)> = sorted_dates
        .par_iter()
        .map(|date| {
            let wide_raw_path = ticker.raw_wide_path("backtest", date);
            let wide_raw = match super::cache::read_raw_gz::<CachedRawTheta>(&wide_raw_path) {
                Some(raw) if !raw.oi.is_empty() || !raw.all_greeks.is_empty() => raw,
                _ => return (vec![], false),
            };
            let bars_1m = match bars_by_date.get(date.as_str()) {
                Some(b) if !b.is_empty() => b,
                _ => return (vec![], false),
            };
            let bars_15m = crate::data::hist::resample_bars(bars_1m, interval);
            match super::gex_builder::build_gex_from_wide(ticker, date, &wide_raw, &bars_15m) {
                Ok(gex_map) => {
                    (super::thetadata_hist::gex_map_to_cached_entries(&gex_map), true)
                }
                Err(e) => {
                    eprintln!("[migrate] {} {}: gex build failed: {}", ticker, date, e);
                    (vec![], false)
                }
            }
        })
        .collect();

    let mut all_entries: Vec<CachedGexEntry> = Vec::new();
    let mut days_built = 0u32;
    for (entries, ok) in results {
        if ok { days_built += 1; }
        all_entries.extend(entries);
    }

    let processed_months: HashSet<String> = sorted_dates
        .iter()
        .filter_map(|d| d.get(..7).map(|s| s.to_string()))
        .collect();

    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    match write_all_gex(ticker, &all_entries, &processed_months) {
        Ok(()) => eprintln!(
            "[migrate] {} gex: {} entries from {} days ({} months)",
            ticker, all_entries.len(), days_built, processed_months.len(),
        ),
        Err(e) => eprintln!("[migrate] {} gex: write failed: {}", ticker, e),
    }
}

// ─── GEX ────────────────────────────────────────────────────────────────────

pub fn read_all_gex(ticker: Ticker) -> Option<Vec<CachedGexEntry>> {
    let path = gex_parquet_path(ticker.as_str());
    let entries = super::parquet_cache::read_gex(&path)?;
    let months = super::parquet_cache::read_metadata_set(&path, "processed_months");
    GEX_MONTHS_CACHE.lock().unwrap().insert(ticker, months);
    Some(entries)
}

pub fn write_all_gex(ticker: Ticker, entries: &[CachedGexEntry], processed_months: &HashSet<String>) -> Result<()> {
    super::parquet_cache::write_gex(&gex_parquet_path(ticker.as_str()), entries, processed_months)
}

/// Record a month's GEX. `month` is always marked processed (even if empty).
pub fn append_month_gex(ticker: Ticker, month: &str, month_entries: &[CachedGexEntry]) -> Result<()> {
    let path = gex_parquet_path(ticker.as_str());
    let mut months = super::parquet_cache::read_metadata_set(&path, "processed_months");
    if months.contains(month) {
        return Ok(());
    }
    months.insert(month.to_string());
    let mut all = super::parquet_cache::read_gex(&path).unwrap_or_default();
    if !month_entries.is_empty() {
        let existing: HashSet<i64> = all.iter().map(|e| e.timestamp).collect();
        let new_entries: Vec<_> = month_entries.iter()
            .filter(|e| !existing.contains(&e.timestamp))
            .cloned()
            .collect();
        all.extend(new_entries);
        all.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    }
    super::parquet_cache::write_gex(&path, &all, &months)?;
    GEX_MONTHS_CACHE.lock().unwrap().insert(ticker, months);
    Ok(())
}

/// Remove a month's bars from the cache (for testing cache rebuild).
pub fn remove_month_bars(ticker: Ticker, month: &str) -> Result<()> {
    let path = bars_parquet_path(ticker.as_str());
    let mut dates = super::parquet_cache::read_metadata_set(&path, "processed_dates");
    let mut bars = super::parquet_cache::read_bars(&path).unwrap_or_default();
    let before = bars.len();
    dates.retain(|d| !d.starts_with(month));
    let (m_start, m_end) = month_epoch_range(month);
    bars.retain(|b| b.timestamp < m_start || b.timestamp >= m_end);
    if bars.len() == before {
        return Ok(());
    }
    super::parquet_cache::write_bars(&path, &bars, &dates)?;
    BAR_DATES_CACHE.lock().unwrap().remove(&ticker);
    Ok(())
}

pub fn remove_month_gex(ticker: Ticker, month: &str) -> Result<()> {
    let path = gex_parquet_path(ticker.as_str());
    let mut months = super::parquet_cache::read_metadata_set(&path, "processed_months");
    let mut entries = super::parquet_cache::read_gex(&path).unwrap_or_default();
    let before = entries.len();
    months.remove(month);
    let (m_start, m_end) = month_epoch_range(month);
    entries.retain(|e| e.timestamp < m_start || e.timestamp >= m_end);
    if entries.len() == before {
        return Ok(());
    }
    super::parquet_cache::write_gex(&path, &entries, &months)?;
    GEX_MONTHS_CACHE.lock().unwrap().remove(&ticker);
    Ok(())
}

/// Epoch-second range [start, end) for a "YYYY-MM" month string.
fn month_epoch_range(month: &str) -> (i64, i64) {
    let first = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .expect("valid month");
    let start = first.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    let next = if first.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(first.year() + 1, 1, 1).unwrap()
    } else {
        chrono::NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1).unwrap()
    };
    let end = next.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    (start, end)
}

/// Set of YYYY-MM months that have been processed (including empty ones).
/// Result is cached in-process after first read.
pub fn cached_gex_months(ticker: Ticker) -> HashSet<String> {
    {
        let lock = GEX_MONTHS_CACHE.lock().unwrap();
        if let Some(months) = lock.get(&ticker) {
            return months.clone();
        }
    }
    let months = super::parquet_cache::read_metadata_set(
        &gex_parquet_path(ticker.as_str()), "processed_months",
    );
    GEX_MONTHS_CACHE.lock().unwrap().insert(ticker, months.clone());
    months
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parquet_metadata_roundtrip() {
        let dir = std::env::temp_dir().join("gex_parquet_meta_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.parquet");

        let dates: HashSet<String> = ["2024-01-02", "2024-01-03", "2024-07-04"]
            .iter().map(|s| s.to_string()).collect();
        let bars = vec![
            super::super::thetadata_hist::CachedBar {
                timestamp: 1704200400, open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: 100.0,
            },
        ];
        super::super::parquet_cache::write_bars(&path, &bars, &dates).unwrap();
        let loaded = super::super::parquet_cache::read_metadata_set(&path, "processed_dates");
        assert_eq!(dates, loaded);

        fs::remove_dir_all(&dir).ok();
    }

    /// Run with: cargo test --release --lib strip_month -- --ignored --nocapture
    #[test]
    #[ignore]
    fn strip_month() {
        let ticker = crate::config::Ticker::AAPL;
        let month = "2026-03";
        remove_month_bars(ticker, month).unwrap();
        super::super::hist::delete_month_gex(ticker, &format!("{month}-01"));
        let ticker_dir = crate::data::cache::cache_dir().join(ticker.as_str());
        let month_dir = ticker_dir.join(month);
        if let Ok(entries) = std::fs::read_dir(&month_dir) {
            for e in entries.flatten() {
                let raw = e.path().join("raw_options_wide_v5.json.gz");
                if raw.exists() {
                    std::fs::remove_file(&raw).ok();
                    eprintln!("removed {}", raw.display());
                }
            }
        }
        eprintln!("stripped {month} from AAPL bars + GEX");
    }
}
