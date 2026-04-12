use anyhow::Result;
use std::collections::HashMap;

use super::log_debug;
use super::dashboard_types::TickerHealth;
use super::dashboard::{update_health, SharedHealthState};
use crate::broker::ibkr::fetch_ibkr_historical_bars;
use crate::config::strategy::{HURST_WINDOW, WALL_SMOOTH_HALFLIFE};
use crate::config::{StrategyConfig, Ticker};
use crate::data::gex_builder::profiles_from_wide;
use crate::data::cache::downloads_dir_for;
use crate::data::hist::load_or_fetch_wide_raw;
use crate::strategy::engine::StrategyEngine;
use crate::types::{AsLenU32, GexProfile};

/// Shared context for recovery (constant across all tickers in the init loop).
pub struct RecoveryCtx<'a> {
    pub ibkr_client: &'a ibapi::Client,
    pub health: &'a SharedHealthState,
}

async fn replay_warmup(
    rctx: &RecoveryCtx<'_>,
    ticker: Ticker,
    config: &StrategyConfig,
    engine: &mut StrategyEngine,
    wall_smoother: &mut crate::strategy::wall_smoother::WallSmoother,
    hurst: &mut crate::strategy::hurst::HurstTracker,
    spot: f64,
) -> Result<i64> {
    let today = chrono::Local::now().date_naive();
    let split = config.warmup_day_split(today);
    let warmup_td = split.gex_days.len().as_len_u32();

    // IBKR historical_data with end=None returns only completed sessions;
    // today's partial session is picked up by the live bar loop, not warmup.
    push_warmup(rctx.health, ticker, spot, &format!("Fetching {} trading day bars+GEX…", warmup_td));
    println!("[recovery] {} warmup: {} trading day (completed sessions only)", ticker, warmup_td);
    let bars_with_gex = fetch_ibkr_historical_bars(
        rctx.ibkr_client, ticker, warmup_td, None,
    ).await?;

    // Derive GEX dates from actual IBKR bars — our naive calendar doesn't know
    // about market holidays (Good Friday, etc.) so gex_days can be misaligned.
    use chrono_tz::America::New_York;
    let gex_dates: std::collections::BTreeSet<chrono::NaiveDate> = bars_with_gex.iter()
        .map(|b| b.timestamp.with_timezone(&New_York).date_naive())
        .collect();

    let gex_map = fetch_gex_profiles(ticker, rctx.health, spot, &gex_dates).await;

    push_warmup(rctx.health, ticker, spot, &format!("Replaying {} bars…", bars_with_gex.len()));
    let result = engine.warm_up(&bars_with_gex, &gex_map, wall_smoother, hurst, config, ticker, true)?;

    println!(
        "[recovery] {} warmup: {} bars ({} signal) — {} GEX profiles",
        ticker, bars_with_gex.len(), result.signal_bars, gex_map.len(),
    );

    Ok(result.last_processed_ms)
}

/// Fetch ThetaData wide data for exact dates and build GexProfiles keyed by epoch seconds.
async fn fetch_gex_profiles(
    ticker: Ticker,
    health: &SharedHealthState,
    spot: f64,
    dates: &std::collections::BTreeSet<chrono::NaiveDate>,
) -> HashMap<i64, GexProfile> {
    push_warmup(health, ticker, spot, "Fetching GEX history…");

    let mut gex_map: HashMap<i64, GexProfile> = HashMap::new();
    let mut cached = 0usize;
    for &date in dates {
        let date_str = date.format(crate::types::DATE_FMT).to_string();
        let cache_path = ticker.raw_wide_path("live", &date_str);
        let was_cached = cache_path.exists();
        let raw = load_or_fetch_wide_raw(ticker, date, &cache_path).await;
        if was_cached && raw.is_some() { cached += 1; }
        if let Some(raw) = raw {
            for (key, profile) in profiles_from_wide(ticker, &raw) {
                gex_map.insert(key, profile);
            }
        }
    }

    if gex_map.is_empty() {
        println!("[recovery] {} ThetaData GEX history unavailable — signal state starts cold", ticker);
    } else if cached > 0 {
        println!("[recovery] {} GEX warmup: {} days from cache", ticker, cached);
    }

    gex_map
}

fn push_warmup(health: &SharedHealthState, ticker: Ticker, spot: f64, status: &str) {
    update_health(health, ticker.as_str(), TickerHealth {
        last_poll_ms: crate::types::now_ms(),
        spot_price: spot,
        warmup_status: Some(status.to_string()),
        ..Default::default()
    });
}

/// Remove live cache directories older than the GEX warmup window.
/// Only touches data/cache/live/ — never backtest cache.
fn cleanup_stale_live_cache(config: &StrategyConfig) {
    let live_dir = downloads_dir_for("live");
    let Ok(tickers) = std::fs::read_dir(&live_dir) else { return };
    let today = chrono::Local::now().date_naive();
    let max_age = chrono::Duration::days(i64::from(config.warmup_gex_calendar_days()) + 1);
    let mut removed = 0usize;

    for ticker_entry in tickers.flatten() {
        if !ticker_entry.file_type().is_ok_and(|ft| ft.is_dir()) { continue; }
        let Ok(dates) = std::fs::read_dir(ticker_entry.path()) else { continue };
        for date_entry in dates.flatten() {
            let name = date_entry.file_name();
            let date_str = name.to_string_lossy();
            let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, crate::types::DATE_FMT) else { continue };
            if today - date > max_age && std::fs::remove_dir_all(date_entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        println!("[recovery] cleaned up {removed} stale live cache directories");
    }
}

/// Warmed engine + smoother + Hurst + last bar cursor from [`startup_recovery`].
pub struct RecoveredSignalPipeline {
    pub engine: StrategyEngine,
    pub wall_smoother: crate::strategy::wall_smoother::WallSmoother,
    pub hurst: crate::strategy::hurst::HurstTracker,
    pub last_processed_ms: i64,
}

/// Load saved state (if any) and replay historical bars to warm up the full
/// signal pipeline. Returns a ready-to-use StrategyEngine and the last
/// processed bar timestamp.
pub async fn startup_recovery(
    ticker: Ticker,
    config: &StrategyConfig,
    rctx: &RecoveryCtx<'_>,
    saved: Option<&super::state::LiveTradingState>,
    spot: f64,
) -> Result<RecoveredSignalPipeline> {
    cleanup_stale_live_cache(config);

    let mut engine = StrategyEngine::new(config);
    let mut wall_smoother = crate::strategy::wall_smoother::WallSmoother::with_spread_halflife(WALL_SMOOTH_HALFLIFE, config.spread_smooth_halflife);
    let mut hurst = crate::strategy::hurst::HurstTracker::new(HURST_WINDOW);

    if let Some(s) = saved {
        log_debug!(
            "[live-{}] Resuming from saved state (last bar: {})",
            ticker, s.last_bar_timestamp
        );
        engine.signal_state.holding = s.signal_state.holding;
        engine.signal_state.entry_price = s.signal_state.entry_price;
    } else {
        log_debug!(
            "[live-{}] No saved state, starting fresh with warmup replay...",
            ticker
        );
    }

    let recovery_ms = replay_warmup(
        rctx, ticker, config, &mut engine, &mut wall_smoother, &mut hurst, spot,
    ).await?;

    let last_ms = if let Some(s) = saved {
        chrono::DateTime::parse_from_rfc3339(&s.last_bar_timestamp)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(recovery_ms)
    } else {
        recovery_ms
    };

    Ok(RecoveredSignalPipeline {
        engine,
        wall_smoother,
        hurst,
        last_processed_ms: last_ms,
    })
}
