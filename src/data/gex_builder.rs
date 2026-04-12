use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;

use crate::backtest::splits::split_ratio_for_date;
use crate::config::Ticker;
use crate::types::{
    GexProfile, OhlcBar, OptionContract, OptionsSnapshot, WallLevel, option_row_valid, strike_key_to_f64, MIN_IV,
    MAX_IV,
};

use super::thetadata_hist::{
    build_oi_map, contract_key, parse_theta_timestamp, ts_key, CachedRawTheta,
};

/// Convert a strike→γ×OI map into sorted `WallLevel` vec, keeping top `n`.
fn top_n_walls(by_strike: &HashMap<i64, f64>, n: usize) -> Vec<WallLevel> {
    let mut walls: Vec<WallLevel> = by_strike
        .iter()
        .map(|(&sk, &gamma_oi)| WallLevel { strike: strike_key_to_f64(sk), gamma_oi })
        .collect();
    walls.sort_by(|a, b| crate::types::cmp_f64(b.gamma_oi, a.gamma_oi));
    walls.truncate(n);
    walls
}

/// Compute walls filtered by minimum OTM distance from spot.
/// Top 5 by gamma×OI per side. Used by both wide (3% OTM) and mid (1% OTM).
/// Shared by backtest (gex_builder) and live (thetadata_live).
pub fn compute_filtered_walls(
    contracts: &[OptionContract],
    spot: f64,
    min_otm_pct: f64,
) -> (Vec<WallLevel>, Vec<WallLevel>) {
    let mut by_strike_call: HashMap<i64, f64> = HashMap::new();
    let mut by_strike_put: HashMap<i64, f64> = HashMap::new();

    for c in contracts {
        if c.oi <= 0.0 || c.gamma <= 0.0 {
            continue;
        }
        let gamma_oi = c.gamma * c.oi * 100.0;
        let sk = crate::types::strike_key(c.strike);
        if c.is_call {
            if c.strike > spot * (1.0 + min_otm_pct) {
                *by_strike_call.entry(sk).or_insert(0.0) += gamma_oi;
            }
        } else if c.strike < spot * (1.0 - min_otm_pct) {
            *by_strike_put.entry(sk).or_insert(0.0) += gamma_oi;
        }
    }

    (top_n_walls(&by_strike_put, 5), top_n_walls(&by_strike_call, 5))
}

/// Backward-compat wrapper: wide walls at 3% OTM.
pub fn compute_wide_walls(contracts: &[OptionContract], spot: f64) -> (Vec<WallLevel>, Vec<WallLevel>) {
    compute_filtered_walls(contracts, spot, 0.03)
}

/// Compute ATM put IV: max IV of puts within ±5% of spot, excluding extreme values.
/// Shared by backtest (gex_builder) and live (thetadata_live).
pub fn compute_atm_put_iv(contracts: &[OptionContract], spot: f64) -> Option<f64> {
    let lo = spot * 0.95;
    let hi = spot * 1.05;
    let max_iv = contracts.iter()
        .filter(|c| !c.is_call && c.iv > MIN_IV && c.iv <= MAX_IV && c.strike >= lo && c.strike <= hi)
        .map(|c| c.iv)
        .fold(f64::NEG_INFINITY, f64::max);
    if max_iv > 0.0 { Some(max_iv) } else { None }
}

impl GexProfile {
    /// Enrich with wide walls, trail walls, ATM put IV, and gamma aggregates.
    pub fn enrich_from_contracts(&mut self, contracts: &[OptionContract], spot: f64) {
        let (wpw, wcw) = compute_wide_walls(contracts, spot);
        self.wide_put_walls = wpw;
        self.wide_call_walls = wcw;

        self.atm_put_iv = compute_atm_put_iv(contracts, spot);

        compute_gamma_aggregates(self, contracts, spot);
    }
}

/// Compute aggregate gamma features from the full options chain.
fn compute_gamma_aggregates(profile: &mut GexProfile, contracts: &[OptionContract], spot: f64) {
    if spot <= 0.0 { return; }

    let ncw = profile.cw();
    let mut total_put_goi = 0.0_f64;
    let mut total_call_goi = 0.0_f64;
    let mut call_goi_above_cw = 0.0_f64;
    let mut pw_weighted_strike = 0.0_f64;
    let mut pw_near_3 = 0.0_f64;
    let mut pw_far = 0.0_f64;
    let mut cw_near_3 = 0.0_f64;
    let mut atm_goi = 0.0_f64;

    for c in contracts {
        if c.oi <= 0.0 || c.gamma <= 0.0 { continue; }
        let goi = c.gamma * c.oi * 100.0;
        let dist_pct = (c.strike - spot) / spot;

        if c.is_call && c.strike > spot {
            total_call_goi += goi;
            if dist_pct <= 0.03 { cw_near_3 += goi; }
            if ncw > 0.0 && c.strike > ncw { call_goi_above_cw += goi; }
        } else if !c.is_call && c.strike < spot {
            total_put_goi += goi;
            pw_weighted_strike += c.strike * goi;
            let below_pct = (spot - c.strike) / spot;
            if below_pct <= 0.03 { pw_near_3 += goi; }
            if below_pct > 0.03 && below_pct <= 0.08 { pw_far += goi; }
        }

        if dist_pct.abs() <= 0.01 { atm_goi += goi; }
    }

    let total_goi = total_put_goi + total_call_goi;

    profile.total_put_goi = total_put_goi;
    profile.total_call_goi = total_call_goi;

    profile.cw_depth_ratio = if total_call_goi > 0.0 {
        call_goi_above_cw / total_call_goi
    } else { 0.0 };

    profile.pw_com_dist_pct = if total_put_goi > 0.0 {
        (spot - pw_weighted_strike / total_put_goi) / spot * 100.0
    } else { 0.0 };

    profile.pw_near_far_ratio = if pw_far > 0.0 { pw_near_3 / pw_far } else { 0.0 };

    profile.atm_gamma_dominance = if total_goi > 0.0 { atm_goi / total_goi } else { 0.0 };

    profile.near_gamma_imbalance = if total_goi > 0.0 {
        (pw_near_3 - cw_near_3) / total_goi
    } else { 0.0 };

    profile.gamma_tilt = if total_goi > 0.0 {
        (total_call_goi - total_put_goi) / total_goi
    } else { 0.0 };
}

/// Build an OptionContract from an AllGreeksRow + OI + reference date.
/// Returns None if the row is invalid or expired.
pub fn contract_from_greeks_row(
    row: &super::thetadata::AllGreeksRow,
    oi: f64,
    ref_date: chrono::NaiveDate,
) -> Option<OptionContract> {
    if !option_row_valid(row.underlying_price, row.gamma, row.implied_vol, oi, row.strike) {
        return None;
    }
    let exp_date = chrono::NaiveDate::parse_from_str(&row.expiration, crate::types::DATE_FMT).ok()?;
    let dte_days = (exp_date - ref_date).num_days();
    if dte_days < 0 { return None; }
    let expiry = exp_date.and_hms_opt(0, 0, 0).expect("midnight is always valid").and_utc();
    Some(OptionContract {
        symbol: super::thetadata_hist::contract_key(&row.expiration, row.strike, &row.right),
        strike: row.strike,
        expiry,
        is_call: row.is_call(),
        oi,
        gamma: row.gamma,
        iv: row.implied_vol,
        vanna: row.vanna,
        delta: row.delta,
        vega: row.vega,
    })
}

/// Parsed wide data grouped by bar timestamp.
struct WideBarGroups {
    by_bar: HashMap<i64, Vec<OptionContract>>,
    spot_by_bar: HashMap<i64, f64>,
}

/// Parse CachedRawTheta into per-bar contract groups. Shared by
/// `build_gex_from_wide` (backtest) and `profiles_from_wide` (live warmup).
fn parse_wide_contracts(
    wide_raw: &CachedRawTheta,
) -> Option<WideBarGroups> {
    let oi_map = build_oi_map(&wide_raw.oi);
    if oi_map.is_empty() || wide_raw.all_greeks.is_empty() {
        return None;
    }

    let mut by_bar: HashMap<i64, Vec<OptionContract>> = HashMap::new();
    let mut spot_by_bar: HashMap<i64, f64> = HashMap::new();

    for row in &wide_raw.all_greeks {
        let ts = match parse_theta_timestamp(&row.timestamp) {
            Some(dt) => dt,
            None => continue,
        };
        let key = ts_key(&ts);
        let oi = oi_map.get(&contract_key(&row.expiration, row.strike, &row.right))
            .copied().unwrap_or(0.0);
        if let Some(c) = contract_from_greeks_row(row, oi, ts.date_naive()) {
            spot_by_bar.entry(key).or_insert(row.underlying_price);
            by_bar.entry(key).or_default().push(c);
        }
    }

    Some(WideBarGroups { by_bar, spot_by_bar })
}

fn make_snapshot(
    ticker: Ticker,
    epoch: i64,
    spot: f64,
    contracts: &[OptionContract],
) -> OptionsSnapshot {
    OptionsSnapshot {
        timestamp: DateTime::from_timestamp(epoch, 0).unwrap_or_else(Utc::now),
        underlying: ticker,
        spot,
        contracts: contracts.to_vec(),
    }
}

/// Build enriched GexProfiles from parsed wide bar groups.
fn build_profiles(
    ticker: Ticker,
    groups: &WideBarGroups,
    spot_overrides: &HashMap<i64, f64>,
    keys: &[i64],
) -> Vec<(i64, GexProfile)> {
    let mut out = Vec::with_capacity(keys.len());
    for &key in keys {
        let spot = spot_overrides.get(&key)
            .or_else(|| groups.spot_by_bar.get(&key))
            .copied()
            .filter(|&s| s > 0.0);
        let spot = match spot { Some(s) => s, None => continue };
        let contracts = match groups.by_bar.get(&key) {
            Some(c) => c,
            None => continue,
        };
        let snapshot = make_snapshot(ticker, key, spot, contracts);
        let mut gex = snapshot.compute_gex_profile(false);
        gex.enrich_from_contracts(contracts, spot);
        out.push((key, gex));
    }
    out
}

/// Build full enriched GexProfiles from wide data using greeks' own spot.
/// Returns (epoch_sec, GexProfile) pairs in chronological order.
/// Used by live warmup to feed `generate_signal` without needing OHLCV bars for spot.
pub fn profiles_from_wide(
    ticker: Ticker,
    wide_raw: &CachedRawTheta,
) -> Vec<(i64, GexProfile)> {
    let groups = match parse_wide_contracts(wide_raw) {
        Some(g) => g,
        None => return vec![],
    };
    let mut keys: Vec<i64> = groups.spot_by_bar.keys().copied().collect();
    keys.sort();
    let empty = HashMap::new();
    build_profiles(ticker, &groups, &empty, &keys)
}

/// Build complete GEX profiles from wide 15-min data + OHLCV bars.
/// Uses bar.open (split-adjusted) as spot instead of greeks' underlying_price.
pub fn build_gex_from_wide(
    ticker: Ticker,
    date: &str,
    wide_raw: &CachedRawTheta,
    bars: &[OhlcBar],
) -> Result<HashMap<i64, GexProfile>> {
    let groups = match parse_wide_contracts(wide_raw) {
        None => {
            eprintln!("[gex-wide] {} {}: no all_greeks in wide data, skipping", ticker, date);
            return Ok(HashMap::new());
        }
        Some(g) => g,
    };

    let split_ratio = NaiveDate::parse_from_str(date, crate::types::DATE_FMT)
        .map(|d| split_ratio_for_date(ticker.as_str(), d))
        .unwrap_or(1.0);

    let spot_by_time: HashMap<i64, f64> = bars
        .iter()
        .map(|b| (ts_key(&b.timestamp), b.open * split_ratio))
        .collect();

    let mut bar_keys: Vec<i64> = spot_by_time.keys().copied().collect();
    bar_keys.sort();

    let t0 = std::time::Instant::now();
    let profiles = build_profiles(ticker, &groups, &spot_by_time, &bar_keys);
    let result: HashMap<i64, GexProfile> = profiles.into_iter().collect();

    let elapsed = t0.elapsed().as_secs_f64();
    println!(
        "[gex-wide] {} {}: {} profiles from wide data ({:.1}s)",
        ticker, date, result.len(), elapsed
    );

    Ok(result)
}

