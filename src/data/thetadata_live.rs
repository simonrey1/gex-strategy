use anyhow::Result;
use chrono::NaiveDate;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

use crate::config::strategy::OPTION_MAX_EXPIRY_DAYS;
use crate::config::Ticker;
use crate::live::log_debug;
use crate::live::nyse_session::NyseSession;
use crate::types::{AsLenU32, GexPhase, GexProfile, OptionContract, OptionsSnapshot};

use super::thetadata::ThetaClient;
use super::thetadata_hist::{build_oi_map, contract_key};

// ─── Shared state ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LiveGexInner {
    pub phase: GexPhase,
    pub oi_count: u32,
    pub greeks_count: u32,
    pub last_poll_ms: u64,
    pub last_error: Option<String>,
    /// Pre-built GEX profiles keyed by ticker, updated each poll cycle.
    pub profiles: HashMap<Ticker, GexProfile>,
    /// Per-ticker daily OI cache: (date, oi_map). Fetched once per day.
    pub oi_cache: HashMap<Ticker, (NaiveDate, HashMap<String, f64>)>,
}

pub type SharedLiveGex = Arc<Mutex<LiveGexInner>>;

pub fn new_live_gex() -> SharedLiveGex {
    Arc::new(Mutex::new(LiveGexInner {
        phase: GexPhase::Idle,
        oi_count: 0,
        greeks_count: 0,
        last_poll_ms: 0,
        last_error: None,
        profiles: HashMap::new(),
        oi_cache: HashMap::new(),
    }))
}

// ─── Status (for health endpoint) ───────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct GexStreamStatus {
    pub phase: GexPhase,
    #[serde(rename = "oiUpdates")]
    pub oi_updates: u32,
    #[serde(rename = "greeksUpdates")]
    pub greeks_updates: u32,
    #[serde(rename = "lastPollMs")]
    pub last_poll_ms: u64,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    #[serde(rename = "accelReady")]
    pub accel_ready: u32,
    #[serde(rename = "accelTotal")]
    pub accel_total: u32,
    /// Current sample count (max across tickers) vs minimum needed.
    #[serde(rename = "accelSamples")]
    pub accel_samples: u32,
    #[serde(rename = "accelMinSamples")]
    pub accel_min_samples: u32,
}

pub fn get_gex_status(state: &SharedLiveGex) -> GexStreamStatus {
    let s = crate::types::lock_or_recover(state);
    let accel_total = s.profiles.len().as_len_u32();
    GexStreamStatus {
        phase: s.phase,
        oi_updates: s.oi_count,
        greeks_updates: s.greeks_count,
        last_poll_ms: s.last_poll_ms,
        last_error: s.last_error.clone(),
        accel_ready: accel_total,
        accel_total,
        accel_samples: 0,
        accel_min_samples: 0,
    }
}

/// Get a pre-computed GEX profile for a specific ticker.
pub fn get_live_gex_profile(
    state: &SharedLiveGex,
    ticker: Ticker,
) -> Option<GexProfile> {
    let s = crate::types::lock_or_recover(state);
    if s.phase != GexPhase::Live {
        return None;
    }
    s.profiles.get(&ticker).cloned()
}


// ─── Polling loop ───────────────────────────────────────────────────────────

/// Start the live GEX system: poll ThetaData snapshots every 1 minute for
/// GEX acceleration, while the strategy acts at bar_interval_minutes.
pub async fn start_live_gex(
    state: SharedLiveGex,
    tickers: &[Ticker],
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let now = chrono::Utc::now();

    if !NyseSession::is_open(&now) {
        log_debug!("[live-gex] Market is closed — deferring GEX polling until market open");
        let state_clone = state.clone();
        let tickers_vec: Vec<Ticker> = tickers.to_vec();
        let sd = shutdown.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                if sd.load(Ordering::Relaxed) { return; }
                let now = chrono::Utc::now();
                if NyseSession::is_open(&now) {
                    log_debug!("[live-gex] Market is open — starting GEX polling");
                    run_gex_poll_loop(state_clone, &tickers_vec, &sd).await;
                    return;
                }
            }
        });
        {
            let mut s = crate::types::lock_or_recover(&state);
            s.phase = GexPhase::Idle;
            s.last_error = Some("Market closed — GEX will start at market open".to_string());
        }
        return Ok(());
    }

    // Do an initial poll immediately, then spawn the loop
    let initial_tickers: Vec<Ticker> = tickers.to_vec();
    {
        let mut s = crate::types::lock_or_recover(&state);
        s.phase = GexPhase::Fetching;
    }

    if let Err(e) = poll_once(&state, &initial_tickers).await {
        eprintln!("[live-gex] Initial poll failed: {:?}", e);
        let mut s = crate::types::lock_or_recover(&state);
        s.last_error = Some(format!("Initial poll failed: {:?}", e));
    }

    let state_clone = state.clone();
    let tickers_vec: Vec<Ticker> = tickers.to_vec();
    tokio::spawn(async move {
        run_gex_poll_loop(state_clone, &tickers_vec, &shutdown).await;
    });

    Ok(())
}

async fn run_gex_poll_loop(
    state: SharedLiveGex,
    tickers: &[Ticker],
    shutdown: &AtomicBool,
) {
    let poll_secs = crate::config::bar_interval::poll_interval_secs_u64(crate::config::BAR_INTERVAL_MINUTES);
    loop {
        // Sleep in 1-second ticks so we notice shutdown quickly.
        for _ in 0..poll_secs {
            if shutdown.load(Ordering::Relaxed) { return; }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        if shutdown.load(Ordering::Relaxed) { return; }

        let now = chrono::Utc::now();
        if !NyseSession::is_open(&now) {
            continue;
        }

        if let Err(e) = poll_once(&state, tickers).await {
            eprintln!("[live-gex] Poll error: {:?}", e);
            let mut s = crate::types::lock_or_recover(&state);
            s.last_error = Some(format!("{:?}", e));
            s.phase = GexPhase::Error;
        }
    }
}

async fn poll_once(
    state: &SharedLiveGex,
    tickers: &[Ticker],
) -> Result<()> {
    let client = ThetaClient::from_env();
    let max_dte = Some(OPTION_MAX_EXPIRY_DAYS);
    let today = chrono::Utc::now().date_naive();

    let mut new_profiles: HashMap<Ticker, GexProfile> = HashMap::new();
    let mut total_oi = 0u32;
    let mut total_greeks = 0u32;
    let mut oi_fetched_fresh = false;

    for &ticker in tickers {
        let symbol = ticker.as_str();

        // Wide fetch: all greeks ±25% (no strike_range = full chain), same as backtest.
        // Fetched first so we can extract underlying_price as spot (same source as
        // profiles_from_wide used by live warmup and backtest fallback).
        let all_rows = client
            .option_all_greeks_snapshot(symbol, max_dte, None)
            .await?;

        let spot = all_rows.iter()
            .map(|r| r.underlying_price)
            .find(|&p| p > 0.0)
            .unwrap_or(0.0);
        if spot <= 0.0 || all_rows.is_empty() {
            continue;
        }

        // OI is published once per day — reuse cached if already fetched today
        let cached_oi = {
            let s = crate::types::lock_or_recover(state);
            s.oi_cache.get(&ticker)
                .filter(|(date, _)| *date == today)
                .map(|(_, map)| map.clone())
        };

        let (oi_map, oi_count) = if let Some(map) = cached_oi {
            let count = map.len().as_len_u32();
            (map, count)
        } else {
            let oi_rows = client.option_oi_snapshot(symbol, max_dte, None).await?;
            let count = oi_rows.len().as_len_u32();
            let map = build_oi_map(&oi_rows);
            oi_fetched_fresh = true;
            println!("[live-gex] {} OI fetched: {} contracts (cached for {})", ticker, count, today);
            {
                let mut s = crate::types::lock_or_recover(state);
                s.oi_cache.insert(ticker, (today, map.clone()));
            }
            (map, count)
        };

        total_oi += oi_count;
        total_greeks += all_rows.len().as_len_u32();

        let now = chrono::Utc::now();
        let ref_date = now.date_naive();
        let mut contract_map: HashMap<String, OptionContract> = HashMap::new();
        for row in &all_rows {
            let key = contract_key(&row.expiration, row.strike, &row.right);
            let oi = oi_map.get(&key).copied().unwrap_or(0.0);
            if let Some(c) = super::gex_builder::contract_from_greeks_row(
                row, oi, ref_date,
            ) {
                contract_map.insert(key, c);
            }
        }
        let contracts: Vec<OptionContract> = contract_map.into_values().collect();

        if contracts.is_empty() {
            continue;
        }

        let snapshot = OptionsSnapshot {
            timestamp: now,
            underlying: ticker,
            spot,
            contracts: contracts.clone(),
        };
        let mut profile = snapshot.compute_gex_profile(false);
        profile.enrich_from_contracts(&contracts, spot);

        new_profiles.insert(ticker, profile);
    }

    let now_ts = chrono::Utc::now();
    let now_ms = crate::types::datetime_millis_u64(&now_ts);

    let profile_count = new_profiles.len();
    let no_data = profile_count == 0;
    {
        let mut s = crate::types::lock_or_recover(state);
        s.profiles = new_profiles;
        s.oi_count = total_oi;
        s.greeks_count = total_greeks;
        s.last_poll_ms = now_ms;
        if no_data {
            s.phase = GexPhase::Error;
            s.last_error = Some(format!(
                "0/{} tickers returned GEX data — walls unavailable",
                tickers.len()
            ));
        } else {
            s.phase = GexPhase::Live;
            s.last_error = None;
        }
    }

    if no_data {
        eprintln!(
            "[live-gex] Poll failed: 0/{} tickers returned GEX data @ {}",
            tickers.len(),
            now_ts.format("%H:%M:%S UTC"),
        );
    } else {
        println!(
            "[live-gex] Poll complete: {} greeks, {} OI, {}/{} profiles ({}) @ {}",
            total_greeks,
            total_oi,
            profile_count,
            tickers.len(),
            if oi_fetched_fresh { "fresh" } else { "cached" },
            now_ts.format("%H:%M:%S UTC"),
        );
    }

    Ok(())
}
