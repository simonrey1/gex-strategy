use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::strategy::OPTION_MAX_EXPIRY_DAYS;
use crate::config::Ticker;
use crate::strategy::eastern_time::is_edt;
use crate::types::{GexProfile, OhlcBar, WallLevel};

use super::thetadata::{AllGreeksRow, OiRow, SecondOrderGreeksRow, ThetaClient};

// ─── Timestamp helpers ───────────────────────────────────────────────────────

/// Epoch-seconds key for GEX map lookups.
#[inline]
pub fn ts_key(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp()
}

/// Parse a ThetaData timestamp. The API returns naive Eastern Time (no tz marker).
/// We convert to real UTC. Cached data (RFC 3339 with "Z") is already UTC.
pub fn parse_theta_timestamp(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    let ndt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    let offset_hours = if is_edt(ndt.date()) { 4 } else { 5 };
    Some(ndt.and_utc() + chrono::Duration::hours(offset_hours))
}

// ─── Bar serialization (cache format) ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedBar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

pub fn cached_bars_to_ohlc(cached: Vec<CachedBar>) -> Vec<OhlcBar> {
    cached
        .into_iter()
        .filter_map(|b| {
            let ts = DateTime::from_timestamp(b.timestamp, 0)?;
            Some(OhlcBar {
                timestamp: ts,
                open: b.open,
                high: b.high,
                low: b.low,
                close: b.close,
                volume: b.volume,
            })
        })
        .collect()
}

pub fn ohlc_to_cached(bars: &[OhlcBar]) -> Vec<CachedBar> {
    bars.iter()
        .map(|b| CachedBar {
            timestamp: ts_key(&b.timestamp),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        })
        .collect()
}

// ─── GEX cache types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedGexEntry {
    pub timestamp: i64,
    pub spot: f64,
    #[serde(rename = "putWalls")]
    pub put_walls: Vec<CachedWallEntry>,
    #[serde(rename = "callWalls")]
    pub call_walls: Vec<CachedWallEntry>,
    #[serde(rename = "netGex")]
    pub net_gex: f64,
    #[serde(rename = "atmPutIv", default, skip_serializing_if = "Option::is_none")]
    pub atm_put_iv: Option<f64>,
    #[serde(rename = "widePutWalls", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_put_walls: Vec<CachedWallEntry>,
    #[serde(rename = "wideCallWalls", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_call_walls: Vec<CachedWallEntry>,
    #[serde(rename = "pwComDistPct", default)]
    pub pw_com_dist_pct: f64,
    #[serde(rename = "pwNearFarRatio", default)]
    pub pw_near_far_ratio: f64,
    #[serde(rename = "atmGammaDominance", default)]
    pub atm_gamma_dominance: f64,
    #[serde(rename = "nearGammaImbalance", default)]
    pub near_gamma_imbalance: f64,
    #[serde(rename = "totalPutGoi", default)]
    pub total_put_goi: f64,
    #[serde(rename = "totalCallGoi", default)]
    pub total_call_goi: f64,
    #[serde(rename = "cwDepthRatio", default)]
    pub cw_depth_ratio: f64,
    #[serde(rename = "gammaTilt", default)]
    pub gamma_tilt: f64,
    #[serde(rename = "netVanna", default)]
    pub net_vanna: f64,
    #[serde(rename = "netDelta", default)]
    pub net_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedWallEntry {
    pub strike: f64,
    #[serde(rename = "gammaOI")]
    pub gamma_oi: f64,
}

pub fn cached_gex_to_map(cached: Vec<CachedGexEntry>) -> HashMap<i64, GexProfile> {
    let mut result = HashMap::with_capacity(cached.len());
    for entry in cached {
        let w = |v: Vec<CachedWallEntry>| -> Vec<WallLevel> {
            v.into_iter().map(|w| WallLevel { strike: w.strike, gamma_oi: w.gamma_oi }).collect()
        };
        result.insert(
            entry.timestamp,
            GexProfile {
                spot: entry.spot,
                net_gex: entry.net_gex,
                put_walls: w(entry.put_walls),
                call_walls: w(entry.call_walls),
                atm_put_iv: entry.atm_put_iv,
                wide_put_walls: w(entry.wide_put_walls),
                wide_call_walls: w(entry.wide_call_walls),
                pw_com_dist_pct: entry.pw_com_dist_pct,
                pw_near_far_ratio: entry.pw_near_far_ratio,
                atm_gamma_dominance: entry.atm_gamma_dominance,
                near_gamma_imbalance: entry.near_gamma_imbalance,
                total_put_goi: entry.total_put_goi,
                total_call_goi: entry.total_call_goi,
                cw_depth_ratio: entry.cw_depth_ratio,
                gamma_tilt: entry.gamma_tilt,
                net_vanna: entry.net_vanna,
                net_delta: entry.net_delta,
            },
        );
    }
    result
}

pub fn gex_map_to_cached_entries(gex_map: &HashMap<i64, GexProfile>) -> Vec<CachedGexEntry> {
    let mut sorted_keys: Vec<i64> = gex_map.keys().copied().collect();
    sorted_keys.sort();
    sorted_keys
        .iter()
        .map(|&ts| {
            let gex = &gex_map[&ts];
            CachedGexEntry {
                timestamp: ts,
                spot: gex.spot,
                put_walls: gex
                    .put_walls
                    .iter()
                    .map(|w| CachedWallEntry {
                        strike: w.strike,
                        gamma_oi: w.gamma_oi,
                    })
                    .collect(),
                call_walls: gex
                    .call_walls
                    .iter()
                    .map(|w| CachedWallEntry {
                        strike: w.strike,
                        gamma_oi: w.gamma_oi,
                    })
                    .collect(),
                net_gex: gex.net_gex,
                atm_put_iv: gex.atm_put_iv,
                wide_put_walls: gex.wide_put_walls.iter().map(|w| CachedWallEntry { strike: w.strike, gamma_oi: w.gamma_oi }).collect(),
                wide_call_walls: gex.wide_call_walls.iter().map(|w| CachedWallEntry { strike: w.strike, gamma_oi: w.gamma_oi }).collect(),
                pw_com_dist_pct: gex.pw_com_dist_pct,
                pw_near_far_ratio: gex.pw_near_far_ratio,
                atm_gamma_dominance: gex.atm_gamma_dominance,
                near_gamma_imbalance: gex.near_gamma_imbalance,
                total_put_goi: gex.total_put_goi,
                total_call_goi: gex.total_call_goi,
                cw_depth_ratio: gex.cw_depth_ratio,
                gamma_tilt: gex.gamma_tilt,
                net_vanna: gex.net_vanna,
                net_delta: gex.net_delta,
            }
        })
        .collect()
}

// ─── Raw ThetaData cache ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRawTheta {
    pub oi: Vec<OiRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub greeks: Vec<serde_json::Value>,
    #[serde(default)]
    pub second_order: Vec<SecondOrderGreeksRow>,
    /// Full greeks (delta, vega, gamma, etc.) from `greeks/all` endpoint.
    /// Only populated for wide 15-min fetches. Old caches deserialize as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_greeks: Vec<AllGreeksRow>,
}

// ─── ThetaData fetching ─────────────────────────────────────────────────────

/// Fetch full-chain 15-min greeks (all strikes) for wide structural walls.
/// Uses `greeks/all` endpoint to get delta + vega for VEX/net-delta computation.
pub async fn fetch_options_day_wide(
    ticker: Ticker,
    date: NaiveDate,
) -> Result<CachedRawTheta> {
    fetch_options_day_core(ticker, date, "15m", None).await
}

async fn fetch_options_day_core(
    ticker: Ticker,
    date: NaiveDate,
    interval: &str,
    strike_range: Option<u32>,
) -> Result<CachedRawTheta> {
    let client = ThetaClient::from_env();
    let max_dte_days = OPTION_MAX_EXPIRY_DAYS;
    let tag = if strike_range.is_some() { "" } else { " wide" };

    println!("[thetadata] {} {}: fetching OI ({}{})", ticker, date, interval, tag);
    let label = format!("{} {} OI", ticker, date);
    let oi = retry_theta(&label, || client.option_oi_history_range(ticker.as_str(), date, date)).await?;
    println!("[thetadata] {} {}: {} OI records", ticker, date, oi.len());

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let exp_label = format!("{} {} expirations", ticker, date);
    let all_expirations = retry_theta(&exp_label, || client.list_expirations(ticker.as_str())).await?;
    let relevant_expirations: Vec<NaiveDate> = all_expirations
        .into_iter()
        .filter(|exp| {
            let dte = (*exp - date).num_days();
            dte >= 0 && dte <= i64::from(max_dte_days)
        })
        .collect();
    println!(
        "[thetadata] {} {}: {} relevant expirations (all_greeks{})",
        ticker, date, relevant_expirations.len(), tag
    );

    let all_greeks = fetch_expirations_parallel(
        &client, ticker, date, &relevant_expirations,
        strike_range, interval,
    ).await?;

    println!(
        "[thetadata] {} {}: {} OI, {} all_greeks{}",
        ticker, date, oi.len(), all_greeks.len(), tag
    );

    Ok(CachedRawTheta { oi, greeks: Vec::new(), second_order: Vec::new(), all_greeks })
}

// ─── Per-request retry with rate-limit backoff ─────────────────────────────

const GREEKS_MAX_RETRIES: u32 = 6;
/// Max concurrent ThetaData requests (PRO tier = 4 server threads).
const THETA_CONCURRENCY: usize = 4;

fn is_connection_error(msg: &str) -> bool {
    msg.contains("Connection refused")
        || msg.contains("Connection reset")
        || msg.contains("connection reset")
        || msg.contains("Broken pipe")
        || msg.contains("timed out")
}

fn is_retryable(msg: &str) -> bool {
    msg.contains("429")
        || msg.contains("Too Many")
        || msg.contains("500")
        || msg.contains("Internal Server Error")
        || msg.contains("502")
        || msg.contains("503")
        || msg.contains("504")
        || is_connection_error(msg)
}

async fn retry_theta<F, Fut, T>(label: &str, f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_err = None;
    for attempt in 0..GREEKS_MAX_RETRIES {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let msg = format!("{e:#}");
                if !is_retryable(&msg) && attempt == 0 {
                    return Err(e);
                }
                let is_rate_limit = msg.contains("429") || msg.contains("Too Many");
                let is_conn = is_connection_error(&msg);
                let delay_secs = if is_rate_limit {
                    15 * 2u64.pow(attempt.min(3))
                } else if is_conn {
                    5 * 2u64.pow(attempt.min(3))
                } else {
                    2u64.pow(attempt)
                };
                let tag = if is_rate_limit {
                    " (rate-limited)"
                } else if is_conn {
                    " (conn error)"
                } else {
                    ""
                };
                eprintln!(
                    "[thetadata] {} retry {}/{} in {}s{}: {}",
                    label,
                    attempt + 1,
                    GREEKS_MAX_RETRIES,
                    delay_secs,
                    tag,
                    msg.chars().take(100).collect::<String>(),
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{}: retry_theta exhausted", label)))
}

/// Fetch greeks for all expirations with up to `THETA_CONCURRENCY` in-flight at once.
/// "No data" expirations are skipped; fatal errors abort immediately.
async fn fetch_expirations_parallel(
    client: &ThetaClient,
    ticker: Ticker,
    date: NaiveDate,
    expirations: &[NaiveDate],
    strike_range: Option<u32>,
    interval: &str,
) -> Result<Vec<AllGreeksRow>> {
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(THETA_CONCURRENCY));
    let total = expirations.len();

    let mut handles = Vec::with_capacity(total);
    for (i, &exp) in expirations.iter().enumerate() {
        let sem = sem.clone();
        let client = client.clone();
        let interval = interval.to_string();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let t0 = std::time::Instant::now();
            let label = format!("{} {} exp {}", ticker, date, exp);
            let result = retry_theta(&label, || client.option_all_greeks_range(
                ticker.as_str(), exp, date, date, strike_range, &interval,
            )).await;
            let elapsed = t0.elapsed().as_secs_f64();
            (i, exp, result, elapsed, total, ticker)
        }));
    }

    let mut out = Vec::new();
    for handle in handles {
        let (i, exp, result, elapsed, total, ticker) = handle.await
            .map_err(|e| anyhow::anyhow!("thetadata fetch task panicked: {e}"))?;
        match result {
            Ok(rows) => {
                println!(
                    "[thetadata] {} {}: exp {} -> {} all_greeks ({}/{}) {:.1}s",
                    ticker, date, exp, rows.len(), i + 1, total, elapsed
                );
                out.extend(rows);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if msg.contains("472") || msg.contains("No data found") {
                    println!(
                        "[thetadata] {} {}: exp {} -> no data ({}/{}) {:.1}s",
                        ticker, date, exp, i + 1, total, elapsed
                    );
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(out)
}


pub use super::thetadata::{contract_key, build_oi_map};


