use std::collections::HashMap;

use crate::types::{GexProfile, OptionsSnapshot, WallLevel};

const CONTRACT_MULTIPLIER: f64 = 100.0;
const TOP_N: usize = 5;

#[derive(Default, Clone, Copy)]
struct StrikeAccum {
    call_gamma_oi: f64,
    put_gamma_oi: f64,
}


fn accumulate(
    contracts: &[crate::types::OptionContract],
) -> (HashMap<i64, StrikeAccum>, u32, u32) {
    let mut by_strike: HashMap<i64, StrikeAccum> = HashMap::new();
    let mut used: u32 = 0;
    let mut skipped: u32 = 0;
    for c in contracts {
        if c.oi <= 0.0 || c.gamma <= 0.0 {
            skipped += 1;
            continue;
        }
        used += 1;
        let gamma_oi = c.gamma * c.oi * CONTRACT_MULTIPLIER;
        let strike_key = crate::types::strike_key(c.strike);
        let accum = by_strike
            .entry(strike_key)
            .or_default();
        if c.is_call {
            accum.call_gamma_oi += gamma_oi;
        } else {
            accum.put_gamma_oi += gamma_oi;
        }
    }
    (by_strike, used, skipped)
}

fn extract_walls(by_strike: &HashMap<i64, StrikeAccum>) -> (Vec<WallLevel>, Vec<WallLevel>) {
    let mut put_candidates = Vec::new();
    let mut call_candidates = Vec::new();
    for (&strike_key, accum) in by_strike {
        let strike = crate::types::strike_key_to_f64(strike_key);
        if accum.put_gamma_oi > 0.0 {
            put_candidates.push(WallLevel { strike, gamma_oi: accum.put_gamma_oi });
        }
        if accum.call_gamma_oi > 0.0 {
            call_candidates.push(WallLevel { strike, gamma_oi: accum.call_gamma_oi });
        }
    }
    put_candidates.sort_by(|a, b| crate::types::cmp_f64(b.gamma_oi, a.gamma_oi));
    call_candidates.sort_by(|a, b| crate::types::cmp_f64(b.gamma_oi, a.gamma_oi));
    (
        put_candidates.into_iter().take(TOP_N).collect(),
        call_candidates.into_iter().take(TOP_N).collect(),
    )
}

impl OptionsSnapshot {
    /// Build a GEX profile from pre-computed `gamma` (from ThetaData).
    /// All contracts contribute to a single set of put/call walls (top 5 by gamma*OI).
    pub fn compute_gex_profile(&self, verbose: bool) -> GexProfile {
        if self.spot <= 0.0 || self.contracts.is_empty() {
            return GexProfile::empty(self.spot);
        }

        let (by_strike, used, skipped) = accumulate(&self.contracts);

        if verbose {
            println!(
                "[gex] {} contracts used, {} skipped (zero OI/gamma)",
                used, skipped
            );
        }

        let mut strike_gex: Vec<(f64, f64)> = Vec::new();
        let mut net_gex: f64 = 0.0;
        for (&strike_key, accum) in &by_strike {
            let strike = crate::types::strike_key_to_f64(strike_key);
            let gex = (accum.call_gamma_oi - accum.put_gamma_oi) * self.spot * self.spot * 0.01;
            strike_gex.push((strike, gex));
            net_gex += gex;
        }

        let (put_walls, call_walls) = extract_walls(&by_strike);

        strike_gex.sort_by(|a, b| crate::types::cmp_f64(a.0, b.0));

        if verbose {
            let pw: Vec<String> = put_walls.iter().map(|w| format!("${}", w.strike)).collect();
            let cw: Vec<String> = call_walls.iter().map(|w| format!("${}", w.strike)).collect();
            println!(
                "[gex] {} strikes, netGEX={:.0}, putWalls=[{}], callWalls=[{}]",
                strike_gex.len(),
                net_gex,
                if pw.is_empty() { "none".to_string() } else { pw.join(", ") },
                if cw.is_empty() { "none".to_string() } else { cw.join(", ") },
            );
        }

        let (mut net_vanna, mut net_delta) = (0.0f64, 0.0f64);
        for c in &self.contracts {
            if c.oi <= 0.0 { continue; }
            let mult = c.oi * CONTRACT_MULTIPLIER;
            net_vanna += c.vanna * mult;
            let sign = if c.is_call { 1.0 } else { -1.0 };
            net_delta += c.delta * sign * mult;
        }

        let mut total_call_goi = 0.0_f64;
        let mut total_put_goi = 0.0_f64;
        for accum in by_strike.values() {
            total_call_goi += accum.call_gamma_oi;
            total_put_goi += accum.put_gamma_oi;
        }
        let total_goi = total_call_goi + total_put_goi;
        let gamma_tilt = if total_goi > 0.0 {
            (total_call_goi - total_put_goi) / total_goi
        } else { 0.0 };

        GexProfile {
            spot: self.spot,
            net_gex,
            put_walls,
            call_walls,
            atm_put_iv: None,
            wide_put_walls: vec![],
            wide_call_walls: vec![],
            pw_com_dist_pct: 0.0,
            pw_near_far_ratio: 0.0,
            atm_gamma_dominance: 0.0,
            near_gamma_imbalance: 0.0,
            total_put_goi: 0.0,
            total_call_goi: 0.0,
            cw_depth_ratio: 0.0,
            gamma_tilt,
            net_vanna,
            net_delta,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OptionContract;
    use chrono::TimeZone;

    fn make_contract(strike: f64, is_call: bool, oi: f64, gamma: f64) -> OptionContract {
        let delta = if is_call { 0.5 } else { -0.5 };
        OptionContract {
            symbol: "AAPL".to_string(),
            expiry: chrono::Utc.with_ymd_and_hms(2025, 3, 21, 0, 0, 0).unwrap(),
            strike,
            is_call,
            oi,
            gamma,
            iv: 0.25,
            vanna: 0.01,
            delta,
            vega: 0.1,
        }
    }

    fn make_snapshot() -> OptionsSnapshot {
        OptionsSnapshot {
            spot: 230.0,
            timestamp: chrono::Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap(),
            underlying: crate::config::Ticker::AAPL,
            contracts: vec![
                make_contract(220.0, false, 5000.0, 0.015),
                make_contract(225.0, false, 10000.0, 0.020),
                make_contract(230.0, true, 8000.0, 0.025),
                make_contract(235.0, true, 12000.0, 0.018),
                make_contract(240.0, true, 6000.0, 0.010),
            ],
        }
    }

    #[test]
    fn empty_for_zero_spot() {
        let mut snap = make_snapshot();
        snap.spot = 0.0;
        let p = snap.compute_gex_profile(false);
        assert_eq!(p.net_gex, 0.0);
        assert!(p.put_walls.is_empty());
        assert!(p.call_walls.is_empty());
    }

    #[test]
    fn empty_for_no_contracts() {
        let mut snap = make_snapshot();
        snap.contracts.clear();
        let p = snap.compute_gex_profile(false);
        assert_eq!(p.net_gex, 0.0);
    }

    #[test]
    fn identifies_put_wall() {
        let p = make_snapshot().compute_gex_profile(false);
        assert!(!p.put_walls.is_empty());
        let first = &p.put_walls[0];
        assert!(first.strike == 220.0 || first.strike == 225.0);
    }

    #[test]
    fn identifies_call_wall() {
        let p = make_snapshot().compute_gex_profile(false);
        assert!(!p.call_walls.is_empty());
        let first = &p.call_walls[0];
        assert!([230.0, 235.0, 240.0].contains(&first.strike));
    }

    #[test]
    fn spot_preserved() {
        let p = make_snapshot().compute_gex_profile(false);
        assert!((p.spot - 230.0).abs() < 1e-6);
    }

    #[test]
    fn walls_ranked_by_gamma_oi_descending() {
        let p = make_snapshot().compute_gex_profile(false);
        for i in 1..p.put_walls.len() {
            assert!(p.put_walls[i].gamma_oi <= p.put_walls[i - 1].gamma_oi);
        }
        for i in 1..p.call_walls.len() {
            assert!(p.call_walls[i].gamma_oi <= p.call_walls[i - 1].gamma_oi);
        }
    }

    #[test]
    fn skips_zero_oi() {
        let snap = OptionsSnapshot {
            spot: 230.0,
            timestamp: chrono::Utc::now(),
            underlying: crate::config::Ticker::AAPL,
            contracts: vec![make_contract(230.0, true, 0.0, 0.025)],
        };
        let p = snap.compute_gex_profile(false);
        assert_eq!(p.net_gex, 0.0);
        assert!(p.put_walls.is_empty());
        assert!(p.call_walls.is_empty());
    }

}
