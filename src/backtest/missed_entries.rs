use serde::Serialize;
use ts_rs::TS;

use crate::config::{StrategyConfig, Ticker};
use crate::strategy::eastern_time::et_hhmm_from_epoch;
use crate::types::VfGate;

use super::iv_scan::{IvScanResult, ScanBucket};
use super::state::{BarTooltip, ChartBar, ChartData, SpikeWindow, WallPoint};

const PAD_SEC: i64 = 60 * 60;

/// One gate-blocked "best" entry with a mini chart window around the spike window.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MissedEntry {
    #[serde(flatten)]
    #[ts(flatten)]
    pub result: IvScanResult,
    pub failed_gates: Vec<String>,
    pub sole_gate: Option<String>,
    /// Mini chart bars: spike_start − 20 min … spike_end + 20 min.
    pub bars: Vec<ChartBar>,
    /// Smoothed put wall in the chart window.
    pub smooth_put_wall: Vec<WallPoint>,
    /// Smoothed call wall in the chart window.
    pub smooth_call_wall: Vec<WallPoint>,
    /// Per-bar gate pass/fail tooltips from the backtest runner.
    pub spike_tooltips: Vec<BarTooltip>,
    /// Epoch sec of spike window start (for square marker).
    pub spike_start_sec: i64,
}

/// Aggregate stats per gate for the summary header.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MissedGateSummary {
    pub gate: String,
    pub count: u32,
    pub profit_sum: f64,
    pub sole_count: u32,
    pub sole_profit_sum: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MissedEntriesReport {
    pub total_best: usize,
    pub avg_best_profit: f64,
    pub summaries: Vec<MissedGateSummary>,
    pub entries: Vec<MissedEntry>,
}

fn compute_gate_summaries(best: &[&IvScanResult], cfg: &StrategyConfig) -> Vec<MissedGateSummary> {
    use std::collections::HashMap;
    use crate::strategy::entries::vf_gates::VfCompressParams;

    let mut fails: HashMap<VfGate, (u32, f64, u32, f64)> = HashMap::new();
    for r in best {
        let g = &r.entry_snapshot.gate;
        let compress = VfCompressParams::new(r.entry_snapshot.iv_compression_ratio, cfg);
        let failed: Vec<VfGate> = VfGate::SCAN_GATES.iter()
            .copied()
            .filter(|&gate| !g.check_gate(gate, cfg, &compress))
            .collect();
        let sole = failed.len() == 1;
        for &gate in &failed {
            let e = fails.entry(gate).or_default();
            e.0 += 1;
            e.1 += r.profit_pct;
            if sole { e.2 += 1; e.3 += r.profit_pct; }
        }
    }
    let mut out: Vec<MissedGateSummary> = fails.into_iter()
        .map(|(gate, (c, p, sc, sp))| MissedGateSummary {
            gate: gate.name().to_string(),
            count: c, profit_sum: p, sole_count: sc, sole_profit_sum: sp,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count));
    out
}

trait HasTime: Clone {
    fn time(&self) -> i64;
}
impl HasTime for ChartBar { fn time(&self) -> i64 { self.time } }
impl HasTime for WallPoint { fn time(&self) -> i64 { self.time } }
impl HasTime for BarTooltip { fn time(&self) -> i64 { self.time } }

fn slice_time<T: HasTime>(items: &[T], from: i64, to: i64) -> Vec<T> {
    items.iter()
        .filter(|x| x.time() >= from && x.time() <= to)
        .cloned()
        .collect()
}

fn find_spike_window(windows: &[SpikeWindow], entry_sec: i64) -> Option<&SpikeWindow> {
    windows.iter().find(|w| entry_sec >= w.start && entry_sec <= w.end)
}

fn passes_time_gate(entry_time_sec: i64, cfg: &StrategyConfig) -> bool {
    et_hhmm_from_epoch(entry_time_sec)
        .map(|hhmm| cfg.in_entry_time_window(hhmm))
        .unwrap_or(true)
}

pub fn build_missed_entries_report(
    iv_scan: &[IvScanResult],
    per_ticker_charts: &[(Ticker, ChartData)],
    cfg: &StrategyConfig,
) -> Option<MissedEntriesReport> {
    use crate::strategy::entries::vf_gates::VfCompressParams;

    let best: Vec<&IvScanResult> = iv_scan.iter()
        .filter(|r| r.bucket == ScanBucket::Best && passes_time_gate(r.entry_time_sec, cfg))
        .collect();
    if best.is_empty() { return None; }

    let total_best = best.len();
    let avg_best_profit = best.iter().map(|r| r.profit_pct).sum::<f64>() / total_best as f64;

    let summaries = compute_gate_summaries(&best, cfg);

    let mut entries = Vec::new();

    for r in &best {
        let g = &r.entry_snapshot.gate;
        let compress = VfCompressParams::new(r.entry_snapshot.iv_compression_ratio, cfg);
        let failed: Vec<VfGate> = VfGate::SCAN_GATES.iter()
            .copied()
            .filter(|&gate| !g.check_gate(gate, cfg, &compress))
            .collect();
        if failed.is_empty() { continue; }

        let chart = per_ticker_charts.iter().find(|(t, _)| *t == r.ticker);
        let sw = chart.and_then(|(_, cd)|
            find_spike_window(&cd.spike_windows, r.entry_time_sec)
        );
        let spike_start_sec = sw.map(|w| w.start).unwrap_or(0);
        let (bars, spw, scw, tooltips) = if let Some((_, cd)) = chart {
            let (from_t, to_t) = if let Some(w) = sw {
                (w.start - PAD_SEC, w.end + PAD_SEC)
            } else {
                let half = 60 * 60;
                (r.entry_time_sec - half, r.entry_time_sec + half)
            };
            (
                slice_time(&cd.bars, from_t, to_t),
                slice_time(&cd.smooth_put_wall, from_t, to_t),
                slice_time(&cd.smooth_call_wall, from_t, to_t),
                slice_time(&cd.spike_tooltips, from_t, to_t),
            )
        } else {
            (vec![], vec![], vec![], vec![])
        };

        let sole_gate = if failed.len() == 1 {
            Some(failed[0].name().to_string())
        } else {
            None
        };

        entries.push(MissedEntry {
            result: (*r).clone(),
            failed_gates: failed.iter().map(|g| g.name().to_string()).collect(),
            sole_gate,
            bars,
            smooth_put_wall: spw,
            smooth_call_wall: scw,
            spike_tooltips: tooltips,
            spike_start_sec,
        });
    }

    entries.sort_by(|a, b| b.result.profit_pct.partial_cmp(&a.result.profit_pct).unwrap());

    Some(MissedEntriesReport {
        total_best,
        avg_best_profit,
        summaries,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::iv_scan::ScanSnapshot;
    use crate::strategy::entries::vf_gates::{RegimeCtx, VfGateCtx};

    fn default_gate() -> VfGateCtx {
        VfGateCtx {
            regime: RegimeCtx { tsi: -20.0, adx: 20.0, atr_regime_ratio: 1.0 },
            atr_pct: 0.30,
            slow_atr_pct: 0.35,
            cw_vs_scw_atr: Some(1.0),
            pw_vs_spw_atr: Some(-1.0),
            net_gex: -1e8,
            gex_abs_ema: 1e8,
            bars_since_spike: 5,
            wall_spread_atr: Some(4.0),
            gamma_pos: 0.5,
            spike_vanna: 0.0,
            spike_gamma_tilt: 0.0,
            pw_drift_atr: 0.0,
            net_vanna: 0.0,
            gamma_tilt: 0.0,
            cum_return_atr: 0.0,
        }
    }

    fn default_snapshot() -> ScanSnapshot {
        ScanSnapshot {
            gate: default_gate(),
            spot: 150.0,
            iv_now: 0.25,
            iv_spike_level: 0.40,
            iv_compression_ratio: 0.40,
            pw_dist_atr: Some(-1.0),
            cw_dist_atr: Some(2.0),
            iv_base_ratio: 1.2,
            iv_spike_ratio: 1.8,
            atr_at_spike: 2.0,
            atr_spike_ratio: 0.9,
            cum_iv_drop: -0.10,
            spike_mfe_atr: 2.0,
            spike_mae_atr: -0.5,
        }
    }

    // 2024-03-15 14:50 UTC = 10:50 AM ET (after no_entry_before_et=1030)
    const DEFAULT_ENTRY_SEC: i64 = 1710514200;

    fn make_scan_result(ticker: Ticker, profit: f64, bucket: ScanBucket, gate: VfGateCtx) -> IvScanResult {
        make_scan_result_at(ticker, profit, bucket, gate, DEFAULT_ENTRY_SEC)
    }

    fn make_scan_result_at(ticker: Ticker, profit: f64, bucket: ScanBucket, gate: VfGateCtx, entry_sec: i64) -> IvScanResult {
        let mut snap = default_snapshot();
        snap.gate = gate;
        IvScanResult {
            ticker,
            entry_time: "2024-03-15T14:50:00Z".into(),
            exit_time: "2024-03-20T15:00:00Z".into(),
            entry_price: 150.0,
            exit_price: 150.0 * (1.0 + profit / 100.0),
            atr: 2.0,
            profit_pct: profit,
            max_runup_atr: 3.0,
            exit_reason: "tp".into(),
            entry_time_sec: entry_sec,
            exit_time_sec: 1710943200,
            spike_bars: vec![100],
            bucket,
            snapshot: default_snapshot(),
            entry_snapshot: snap,
        }
    }

    fn make_chart(ticker: Ticker, n_bars: usize, start_sec: i64) -> (Ticker, ChartData) {
        let mut cd = ChartData::default();
        for i in 0..n_bars {
            let t = start_sec + i as i64 * 900;
            cd.bars.push(ChartBar { time: t, open: 150.0, high: 151.0, low: 149.0, close: 150.5 });
            cd.smooth_put_wall.push(WallPoint { time: t, value: 145.0 });
            cd.smooth_call_wall.push(WallPoint { time: t, value: 155.0 });
        }
        (ticker, cd)
    }

    #[test]
    fn empty_scan_returns_none() {
        let cfg = StrategyConfig::default();
        assert!(build_missed_entries_report(&[], &[], &cfg).is_none());
    }

    #[test]
    fn worst_only_returns_none() {
        let cfg = StrategyConfig::default();
        let results = vec![make_scan_result(Ticker::AAPL, 1.0, ScanBucket::Worst, default_gate())];
        assert!(build_missed_entries_report(&results, &[], &cfg).is_none());
    }

    #[test]
    fn best_passing_all_gates_produces_no_entries() {
        let cfg = StrategyConfig::default();
        let results = vec![make_scan_result(Ticker::AAPL, 5.0, ScanBucket::Best, default_gate())];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert_eq!(report.total_best, 1);
        assert_eq!(report.entries.len(), 0);
    }

    #[test]
    fn sole_gate_detected() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0); // fails CwWeak only

        let results = vec![make_scan_result(Ticker::GOOG, 8.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].sole_gate.as_deref(), Some("vf_cw_weak"));
        assert_eq!(report.entries[0].failed_gates, vec!["vf_cw_weak"]);
    }

    #[test]
    fn multiple_gates_no_sole() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0); // fails CwWeak
        gate.wall_spread_atr = Some(50.0); // fails SpreadWide

        let results = vec![make_scan_result(Ticker::AAPL, 10.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
        assert!(report.entries[0].sole_gate.is_none());
        assert!(report.entries[0].failed_gates.len() >= 2);
    }

    #[test]
    fn entries_sorted_by_profit_desc() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let results = vec![
            make_scan_result(Ticker::AAPL, 5.0, ScanBucket::Best, gate),
            make_scan_result(Ticker::GOOG, 15.0, ScanBucket::Best, gate),
            make_scan_result(Ticker::MSFT, 10.0, ScanBucket::Best, gate),
        ];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        let profits: Vec<f64> = report.entries.iter().map(|e| e.result.profit_pct).collect();
        assert_eq!(profits, vec![15.0, 10.0, 5.0]);
    }

    #[test]
    fn chart_window_extracted() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let entry_sec = DEFAULT_ENTRY_SEC;
        let charts = vec![make_chart(Ticker::AAPL, 200, entry_sec - 100 * 900)];
        let results = vec![make_scan_result(Ticker::AAPL, 8.0, ScanBucket::Best, gate)];

        let report = build_missed_entries_report(&results, &charts, &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
        assert!(!report.entries[0].bars.is_empty());
        assert!(!report.entries[0].smooth_put_wall.is_empty());
        assert!(!report.entries[0].smooth_call_wall.is_empty());
    }

    #[test]
    fn no_chart_yields_empty_bars() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let results = vec![make_scan_result(Ticker::AAPL, 8.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert!(report.entries[0].bars.is_empty());
    }

    #[test]
    fn summaries_include_gate_stats() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let results = vec![
            make_scan_result(Ticker::AAPL, 5.0, ScanBucket::Best, gate),
            make_scan_result(Ticker::GOOG, 10.0, ScanBucket::Best, gate),
        ];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert!(!report.summaries.is_empty());
        let cw = report.summaries.iter().find(|s| s.gate == "vf_cw_weak").unwrap();
        assert_eq!(cw.count, 2);
        assert!((cw.profit_sum - 15.0).abs() < 0.01);
    }

    #[test]
    fn slice_time_filters_by_time() {
        let pts = vec![
            WallPoint { time: 10, value: 1.0 },
            WallPoint { time: 20, value: 2.0 },
            WallPoint { time: 30, value: 3.0 },
            WallPoint { time: 40, value: 4.0 },
        ];
        let sliced = slice_time(&pts, 15, 35);
        assert_eq!(sliced.len(), 2);
        assert_eq!(sliced[0].time, 20);
        assert_eq!(sliced[1].time, 30);
    }

    #[test]
    fn iv_scan_result_serializes_time_sec() {
        let r = make_scan_result(Ticker::AAPL, 5.0, ScanBucket::Best, default_gate());
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("entryTimeSec").is_some());
        assert!(json.get("exitTimeSec").is_some());
    }

    #[test]
    fn early_morning_entries_filtered_out() {
        let cfg = StrategyConfig::default(); // no_entry_before_et=1030
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        // 2024-03-15 13:50 UTC = 9:50 AM ET (before 10:30)
        let early_sec = 1710510600_i64;
        let results = vec![
            make_scan_result_at(Ticker::AAPL, 8.0, ScanBucket::Best, gate, early_sec),
        ];
        assert!(build_missed_entries_report(&results, &[], &cfg).is_none());
    }

    #[test]
    fn after_close_entries_filtered_out() {
        let cfg = StrategyConfig::default(); // no_entry_after_et=1500
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        // 2024-03-15 19:15 UTC = 3:15 PM ET (after 15:00)
        let late_sec = 1710530100_i64;
        let results = vec![
            make_scan_result_at(Ticker::AAPL, 8.0, ScanBucket::Best, gate, late_sec),
        ];
        assert!(build_missed_entries_report(&results, &[], &cfg).is_none());
    }

    #[test]
    fn time_gate_passes_midday() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        // DEFAULT_ENTRY_SEC = 10:50 AM ET
        let results = vec![make_scan_result(Ticker::AAPL, 8.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &[], &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
    }

    #[test]
    fn passes_time_gate_unit() {
        let cfg = StrategyConfig::default();
        // 9:50 AM ET
        assert!(!passes_time_gate(1710510600, &cfg));
        // 10:50 AM ET
        assert!(passes_time_gate(DEFAULT_ENTRY_SEC, &cfg));
        // 3:15 PM ET
        assert!(!passes_time_gate(1710530100, &cfg));
    }

    #[test]
    fn find_spike_window_matches_entry_inside() {
        let windows = vec![
            SpikeWindow { start: 1000, end: 2000 },
            SpikeWindow { start: 5000, end: 6000 },
        ];
        let w = find_spike_window(&windows, 1500).unwrap();
        assert_eq!(w.start, 1000);
    }

    #[test]
    fn find_spike_window_none_outside() {
        let windows = vec![SpikeWindow { start: 1000, end: 2000 }];
        assert!(find_spike_window(&windows, 3000).is_none());
    }

    #[test]
    fn find_spike_window_boundary() {
        let windows = vec![SpikeWindow { start: 1000, end: 2000 }];
        assert!(find_spike_window(&windows, 1000).is_some());
        assert!(find_spike_window(&windows, 2000).is_some());
    }

    #[test]
    fn chart_window_anchored_to_spike_window() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let entry_sec = DEFAULT_ENTRY_SEC;
        let spike_start = entry_sec - 30 * 60;
        let spike_end = entry_sec + 20 * 60;

        let (ticker, mut cd) = make_chart(Ticker::AAPL, 200, spike_start - 60 * 60);
        cd.spike_windows.push(SpikeWindow { start: spike_start, end: spike_end });
        let charts = vec![(ticker, cd)];

        let results = vec![make_scan_result(Ticker::AAPL, 8.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &charts, &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
        let e = &report.entries[0];
        assert!(!e.bars.is_empty());
        assert_eq!(e.spike_start_sec, spike_start);
        let first_bar_time = e.bars.first().unwrap().time;
        let last_bar_time = e.bars.last().unwrap().time;
        assert!(first_bar_time <= spike_start - PAD_SEC + 900);
        assert!(last_bar_time >= spike_end + PAD_SEC - 900);
    }

    #[test]
    fn chart_window_fallback_without_spike_window() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0);

        let entry_sec = DEFAULT_ENTRY_SEC;
        let charts = vec![make_chart(Ticker::AAPL, 200, entry_sec - 100 * 900)];
        let results = vec![make_scan_result(Ticker::AAPL, 8.0, ScanBucket::Best, gate)];
        let report = build_missed_entries_report(&results, &charts, &cfg).unwrap();
        assert_eq!(report.entries.len(), 1);
        assert!(!report.entries[0].bars.is_empty());
        assert_eq!(report.entries[0].spike_start_sec, 0);
    }
}
