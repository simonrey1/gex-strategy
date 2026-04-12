use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Serialize;
use ts_rs::TS;

use crate::config::strategy::HURST_WINDOW;
use crate::config::{BarIndex, StrategyConfig, Ticker};
use crate::strategy::entries::iv_eligibility::{IvCompressionInputs, SpikeApplyMut, SpikeCheckInputs};
use crate::strategy::entries::vf_gates::{RegimeCtx, VfCompressParams, VfGateCtx, compute_gamma_pos};
use crate::types::VfGate;
use crate::strategy::entries::BarCtx;
use crate::strategy::engine::{TrailFields, WallTrailOutcome};
use crate::strategy::indicators::IndicatorValues;
use crate::strategy::signals::SignalState;
use crate::strategy::wall_trail::{TrailCheckInputs, check_trail};
use crate::types::{GexProfile, OhlcBar, ToF64, safe_ratio};

use super::metrics::EtFormat;

// ─── Bucket classification ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "lowercase")]
pub enum ScanBucket {
    Best,
    Middle,
    Worst,
}

impl ScanBucket {
    /// Best = profit ≥ 3%, efficient (≥ 0.5%/day), and max drawdown ≤ 3%.
    pub fn classify(exit_profit_pct: f64, days: i64, max_dd_pct: f64) -> Self {
        let days_f = days.max(1).to_f64();
        let efficient = exit_profit_pct / days_f >= 0.5;
        if exit_profit_pct >= 3.0 && efficient && max_dd_pct <= 3.0 { Self::Best }
        else { Self::Worst }
    }

    /// Three-way classification for ML: best / middle (good runup, bad exit) / worst.
    pub fn classify_3way(exit_profit_pct: f64, days: i64, max_dd_pct: f64, max_runup_atr: f64) -> Self {
        let two_way = Self::classify(exit_profit_pct, days, max_dd_pct);
        if two_way == Self::Best { return Self::Best; }
        if max_runup_atr >= 2.0 { Self::Middle } else { Self::Worst }
    }
}

// ─── Snapshot of conditions at compression bar ──────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ScanSnapshot {
    #[serde(flatten)]
    #[ts(flatten)]
    pub gate: VfGateCtx,

    pub spot: f64,
    pub iv_now: f64,
    pub iv_spike_level: f64,
    pub iv_compression_ratio: f64,
    pub pw_dist_atr: Option<f64>,
    pub cw_dist_atr: Option<f64>,
    pub iv_base_ratio: f64,
    pub iv_spike_ratio: f64,
    pub atr_at_spike: f64,
    pub atr_spike_ratio: f64,
    pub cum_iv_drop: f64,
    pub spike_mfe_atr: f64,
    pub spike_mae_atr: f64,
}

struct IvScanSnapCtx<'a> {
    ss: &'a SignalState,
    ind: &'a IndicatorValues,
    gex: &'a GexProfile,
    atr: f64,
    iv_now: f64,
    iv_bl: f64,
    ncw: f64,
    npw: f64,
    spw: f64,
    scw: f64,
    pw_dist: Option<f64>,
    cw_dist: Option<f64>,
    wall_spread: Option<f64>,
    atr_pct: f64,
    slow_atr_pct: f64,
}

impl<'a> IvScanSnapCtx<'a> {
    fn new(ss: &'a SignalState, ind: &'a IndicatorValues, gex: &'a GexProfile) -> Self {
        let atr = ind.atr;
        let iv_now = gex.atm_put_iv_or_zero();
        let iv_bl = ss.iv_baseline_ema;
        let (ncw, npw) = gex.narrow_cw_pw();
        let spw = ss.smoothed_put_wall();
        let scw = ss.spread_call_wall();
        let (pw_dist, cw_dist, wall_spread) = gex.narrow_wall_atr_dists(atr);
        let (atr_pct, slow_atr_pct) = gex.vf_atr_pct_pair(atr, ind.atr_regime_ema);
        Self {
            ss, ind, gex, atr, iv_now, iv_bl, ncw, npw, spw, scw,
            pw_dist, cw_dist, wall_spread, atr_pct, slow_atr_pct,
        }
    }

    /// IV scan snapshot gate — optional CW/PW gaps when narrow walls are missing.
    fn vf_gate(&self) -> VfGateCtx {
        let (cw_vs_scw_atr, pw_vs_spw_atr) =
            self.ss.wall_diff_atr_ref().vf_gate_wall_opts(self.npw, self.ncw, self.spw, self.scw);
        VfGateCtx {
            regime: RegimeCtx::from(self.ind),
            atr_pct: self.atr_pct,
            slow_atr_pct: self.slow_atr_pct,
            cw_vs_scw_atr,
            pw_vs_spw_atr,
            net_gex: self.gex.net_gex,
            gex_abs_ema: self.ss.gex_abs_ema,
            bars_since_spike: self.ss.bars_since_spike(),
            wall_spread_atr: self.wall_spread,
            gamma_pos: compute_gamma_pos(self.gex.spot, self.spw, self.scw),
            spike_vanna: self.ss.spike_net_vanna,
            spike_gamma_tilt: self.ss.spike_gamma_tilt,
            pw_drift_atr: self.ss.pw_drift_atr(self.atr),
            net_vanna: self.gex.net_vanna,
            gamma_tilt: self.gex.gamma_tilt,
            cum_return_atr: self.ss.spike_cum_return_atr,
        }
    }
}

impl<'a> From<IvScanSnapCtx<'a>> for ScanSnapshot {
    fn from(p: IvScanSnapCtx<'a>) -> Self {
        Self {
            gate: p.vf_gate(),
            spot: p.gex.spot,
            iv_now: p.iv_now,
            iv_spike_level: p.ss.iv_spike_level,
            iv_compression_ratio: p.ss.compress_ratio(p.iv_now),
            pw_dist_atr: p.pw_dist,
            cw_dist_atr: p.cw_dist,
            iv_base_ratio: safe_ratio(p.iv_now, p.iv_bl, 1.0),
            iv_spike_ratio: safe_ratio(p.ss.iv_spike_level, p.iv_bl, 1.0),
            atr_at_spike: p.ss.spike_atr,
            atr_spike_ratio: safe_ratio(p.atr, p.ss.spike_atr, 1.0),
            cum_iv_drop: p.ss.spike_cum_iv_drop,
            spike_mfe_atr: p.ss.spike_mfe_atr,
            spike_mae_atr: p.ss.spike_mae_atr,
        }
    }
}

impl ScanSnapshot {
    pub fn capture(ss: &SignalState, ind: &IndicatorValues, gex: &GexProfile) -> Self {
        IvScanSnapCtx::new(ss, ind, gex).into()
    }
}

impl<'a> From<&BarCtx<'a>> for ScanSnapshot {
    #[inline]
    fn from(bctx: &BarCtx<'a>) -> Self {
        Self::capture(bctx.state, bctx.ind, bctx.gex)
    }
}

// ─── Result ─────────────────────────────────────────────────────────────────

/// Best entry per spike — for dashboard markers and JSON state.

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "camelCase")]
pub struct IvScanResult {
    pub ticker: Ticker,
    pub entry_time: String,
    pub exit_time: String,
    pub entry_price: f64,
    pub exit_price: f64,
    pub atr: f64,
    pub profit_pct: f64,
    pub max_runup_atr: f64,
    pub exit_reason: String,
    pub entry_time_sec: i64,
    pub exit_time_sec: i64,
    /// All IV spike bar indices whose scan windows covered this entry.
    #[serde(rename = "spikeBars")]
    pub spike_bars: Vec<BarIndex>,
    pub bucket: ScanBucket,
    pub snapshot: ScanSnapshot,
    /// Conditions captured at the entry bar (not spike bar).
    pub entry_snapshot: ScanSnapshot,
}

// ─── Tracker ────────────────────────────────────────────────────────────────

/// One entry attempt: enters at bar open, tracks SL/TP/wall-trail to exit.
struct ActiveScan {
    ticker: Ticker,
    atr: f64,
    entry_price: f64,
    entry_time: DateTime<Utc>,
    entry_time_sec: i64,
    sl: f64,
    tp: f64,
    highest_put_wall: f64,
    max_high: f64,
    min_low: f64,
    highest_close: f64,
    hurst_exhaust_bars: u32,
    hurst: crate::strategy::hurst::HurstTracker,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    exit_reason: &'static str,
    spike_bar: BarIndex,
    snapshot: ScanSnapshot,
    entry_snapshot: ScanSnapshot,
}

/// Open window: spawns a new ActiveScan on every bar for max_bars bars.
struct ScanWindow {
    ticker: Ticker,
    spike_bar: BarIndex,
    snapshot: ScanSnapshot,
    bars_remaining: BarIndex,
}

struct PendingScan {
    ticker: Ticker,
    spike_bar: BarIndex,
    snapshot: ScanSnapshot,
}

/// Independent per-ticker spike state for the scan oracle.
/// Decoupled from the strategy's holding/entry state so scan events
/// are deterministic regardless of which trades are taken.
#[derive(Default)]
struct ScanSpikeState {
    spike_bar: BarIndex,
    spike_level: f64,
    episode_active: bool,
    bar_index: BarIndex,
}

impl ScanSpikeState {
    fn has_active_spike(&self) -> bool {
        self.spike_bar >= 0 && self.spike_level > 0.0
    }

    fn apply_spike(&mut self, bar_index: BarIndex, sc: &crate::strategy::entries::iv_eligibility::SpikeConditions) {
        sc.apply(
            &mut SpikeApplyMut {
                level: &mut self.spike_level,
                spike_bar: &mut self.spike_bar,
                episode_active: &mut self.episode_active,
            },
            bar_index,
        );
    }
}

pub struct IvScanTracker {
    max_bars: BarIndex,
    pending: Vec<PendingScan>,
    windows: Vec<ScanWindow>,
    active: Vec<ActiveScan>,
    raw_results: Vec<IvScanResult>,
    /// Dedup key: (ticker, bar open time in Unix seconds) — not [`BarIndex`].
    scanned_bars: HashSet<(Ticker, i64)>,
    spike_state: HashMap<Ticker, ScanSpikeState>,
    last_opened_spike: HashMap<Ticker, BarIndex>,
    /// Previous bar's snapshot per ticker (entry decisions use prior bar's close).
    prev_snap: HashMap<Ticker, ScanSnapshot>,
    /// Per-ticker Hurst tracker mirroring the strategy's, so scans start warm.
    hurst_state: HashMap<Ticker, crate::strategy::hurst::HurstTracker>,
}

struct IvScanRunBarCtx<'a> {
    bar: &'a OhlcBar,
    put_wall: f64,
    vol: crate::types::BarVolRegime,
    gex_norm: f64,
    config: &'a StrategyConfig,
}

impl<'a> IvScanRunBarCtx<'a> {
    #[inline]
    fn trail_check_inputs(&self) -> TrailCheckInputs {
        TrailCheckInputs::new(self.put_wall, self.vol, self.gex_norm)
    }
}

impl IvScanTracker {
    pub fn new(max_bars: BarIndex) -> Self {
        Self {
            max_bars, pending: Vec::new(), windows: Vec::new(),
            active: Vec::new(), raw_results: Vec::new(),
            scanned_bars: HashSet::new(),
            spike_state: HashMap::new(),
            last_opened_spike: HashMap::new(),
            prev_snap: HashMap::new(),
            hurst_state: HashMap::new(),
        }
    }

    /// Independent spike detection for the scan oracle.
    /// Uses the same raw inputs as the strategy but ignores holding state.
    /// Returns true + queues a scan if a new spike window should open.
    pub fn detect_and_open(
        &mut self,
        ticker: Ticker,
        bctx: &crate::strategy::entries::BarCtx,
    ) {

        let st = self.spike_state.entry(ticker).or_default();
        st.bar_index = bctx.state.bar_index;

        if let Some(sc) = SpikeCheckInputs::from(bctx).check(bctx.cfg) {
            st.apply_spike(bctx.state.bar_index, &sc);
        }

        if st.has_active_spike() && (st.bar_index - st.spike_bar) > self.max_bars {
            st.spike_level = 0.0;
            st.spike_bar = -1;
        }

        let eligible = IvCompressionInputs {
            has_spike: st.has_active_spike(),
            bars_since_spike: st.bar_index - st.spike_bar,
            atm_put_iv: bctx.atm_put_iv_opt(),
            max_bars: self.max_bars,
            tsi: bctx.ind.tsi,
            elig_tsi_oversold: bctx.cfg.elig_tsi_oversold,
            elig_early_bars: bctx.cfg.elig_early_bars,
        }
        .is_eligible();
        if !eligible {
            return;
        }

        let last = self.last_opened_spike.get(&ticker).copied().unwrap_or(-1);
        if st.spike_bar != last {
            self.last_opened_spike.insert(ticker, st.spike_bar);
            let snap = ScanSnapshot::from(bctx);
            self.pending.push(PendingScan {
                ticker, spike_bar: st.spike_bar, snapshot: snap,
            });
        }
    }

    pub fn update(
        &mut self, ticker: Ticker, put_wall: f64,
        bctx: &crate::strategy::entries::BarCtx,
    ) {
        let bar = bctx.bar;
        let bar_ts = bctx.bar_timestamp_sec();

        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].ticker == ticker {
                let p = self.pending.swap_remove(i);
                self.windows.push(ScanWindow {
                    ticker: p.ticker, spike_bar: p.spike_bar,
                    snapshot: p.snapshot, bars_remaining: self.max_bars,
                });
            } else {
                i += 1;
            }
        }

        let bar_key = (ticker, bar_ts);
        let already_scanned = self.scanned_bars.contains(&bar_key);

        let mut covering_spike_bars: Vec<BarIndex> = Vec::new();
        for w in self.windows.iter_mut() {
            if w.ticker != ticker || w.bars_remaining <= 0 { continue; }
            w.bars_remaining -= 1;
            covering_spike_bars.push(w.spike_bar);
        }

        if !already_scanned && !covering_spike_bars.is_empty() {
            self.scanned_bars.insert(bar_key);
            let w = self.windows.iter()
                .find(|w| w.ticker == ticker)
                .expect("iv_scan: no window for ticker");

            let (sl, tp) = bctx.compute_stops_at_bar_open().expect("iv_scan: compute_stops failed");

            let entry_snap = self.prev_snap.get(&ticker)
                .cloned()
                .unwrap_or_else(|| ScanSnapshot::from(bctx));
            let mut scan = ActiveScan {
                ticker, atr: bctx.ind.atr,
                entry_price: bar.open, entry_time: bar.timestamp, entry_time_sec: bar_ts,
                sl, tp,
                highest_put_wall: 0.0,
                max_high: bar.open,
                min_low: bar.open,
                highest_close: bar.open,
                hurst_exhaust_bars: 0,
                hurst: self.hurst_state.get(&ticker)
                    .cloned()
                    .unwrap_or_else(|| crate::strategy::hurst::HurstTracker::new(HURST_WINDOW)),
                exit_price: bar.close, exit_time: bar.timestamp,
                exit_reason: "end_of_window",
                spike_bar: *covering_spike_bars.first().expect("covering_spike_bars checked non-empty"),
                snapshot: w.snapshot.clone(),
                entry_snapshot: entry_snap,
            };
            Self::run_bar(&mut scan, IvScanRunBarCtx {
                bar,
                put_wall,
                vol: bctx.ind.bar_vol_regime(bar.close),
                gex_norm: bctx.state.gex_norm(bctx.gex.net_gex),
                config: bctx.cfg,
            });
            self.active.push(scan);
        }

        self.windows.retain(|w| w.ticker != ticker || w.bars_remaining > 0);

        for scan in self.active.iter_mut() {
            if scan.ticker != ticker || scan.exit_reason != "end_of_window" { continue; }
            Self::run_bar(scan, IvScanRunBarCtx {
                bar,
                put_wall,
                vol: bctx.ind.bar_vol_regime(bar.close),
                gex_norm: bctx.state.gex_norm(bctx.gex.net_gex),
                config: bctx.cfg,
            });
        }

        // Collect exited scans
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].ticker == ticker && self.active[i].exit_reason != "end_of_window" {
                let scan = self.active.swap_remove(i);
                self.close_scan(scan);
            } else {
                i += 1;
            }
        }

        self.prev_snap.insert(ticker, ScanSnapshot::from(bctx));
        self.hurst_state.entry(ticker)
            .or_insert_with(|| crate::strategy::hurst::HurstTracker::new(HURST_WINDOW))
            .push(bctx.bar.close);
    }

    fn run_bar(scan: &mut ActiveScan, ctx: IvScanRunBarCtx<'_>) {
        let bar = ctx.bar;
        if scan.exit_reason != "end_of_window" { return; }
        if bar.high > scan.max_high { scan.max_high = bar.high; }
        if bar.low < scan.min_low { scan.min_low = bar.low; }

        if let Some((exit_price, reason)) = super::positions::check_stop_loss(bar.open, bar.low, scan.sl) {
            scan.exit_price = exit_price;
            scan.exit_time = bar.timestamp;
            scan.exit_reason = reason;
            return;
        }

        if bar.high >= scan.tp {
            scan.exit_price = scan.tp;
            scan.exit_time = bar.timestamp;
            scan.exit_reason = "take_profit";
            return;
        }

        let tf = TrailFields {
            stop_loss: &mut scan.sl,
            highest_put_wall: &mut scan.highest_put_wall,
            highest_close: &mut scan.highest_close,
            hurst_exhaust_bars: &mut scan.hurst_exhaust_bars,
            entry_price: scan.entry_price,
            tp: scan.tp,
            signal: crate::types::Signal::LongVannaFlip,
        };
        match check_trail(tf, ctx.trail_check_inputs(), ctx.config, &mut scan.hurst) {
            WallTrailOutcome::EarlyTp => {
                scan.exit_price = bar.close;
                scan.exit_time = bar.timestamp;
                scan.exit_reason = "early_tp";
                return;
            }
            WallTrailOutcome::Ratcheted { .. }
            | WallTrailOutcome::Unchanged => {}
        }

        scan.exit_price = bar.close;
        scan.exit_time = bar.timestamp;
    }

    /// Close remaining scans. Returns best entry per spike for dashboard markers / JSON state.
    pub fn finalize(mut self, verbose: bool) -> Vec<IvScanResult> {
        self.windows.clear();
        let remaining: Vec<_> = self.active.drain(..).collect();
        for scan in remaining {
            self.close_scan(scan);
        }

        let mut all = std::mem::take(&mut self.raw_results);
        all.sort_by_key(|r| r.entry_time_sec);

        // Dedup: keep best-profit entry per (ticker, spike_bar) for dashboard
        let mut best_per_spike: HashMap<(Ticker, BarIndex), IvScanResult> = HashMap::new();
        for r in &all {
            for &sb in &r.spike_bars {
                let key = (r.ticker, sb);
                let dominated = best_per_spike.get(&key).is_some_and(|prev| prev.profit_pct >= r.profit_pct);
                if !dominated {
                    let mut entry = r.clone();
                    entry.spike_bars = vec![sb];
                    best_per_spike.insert(key, entry);
                }
            }
        }
        let mut merged: HashMap<(Ticker, i64), IvScanResult> = HashMap::new();
        for r in best_per_spike.into_values() {
            let key = (r.ticker, r.entry_time_sec);
            if let Some(prev) = merged.get_mut(&key) {
                for sb in &r.spike_bars {
                    if !prev.spike_bars.contains(sb) {
                        prev.spike_bars.push(*sb);
                    }
                }
            } else {
                merged.insert(key, r);
            }
        }
        let mut dashboard: Vec<IvScanResult> = merged.into_values().collect();
        dashboard.sort_by_key(|r| r.entry_time_sec);

        if verbose {
            let count = |b: ScanBucket| dashboard.iter().filter(|r| r.bucket == b).count();
            println!(
                "\n[iv-scan] {} best, {} middle (good runup, bad exit), {} worst",
                count(ScanBucket::Best), count(ScanBucket::Middle), count(ScanBucket::Worst),
            );
        }

        dashboard
    }

    fn close_scan(&mut self, scan: ActiveScan) {
        let pct = (scan.exit_price - scan.entry_price) / scan.entry_price * 100.0;
        let max_runup_atr = if scan.atr > 0.0 {
            (scan.max_high - scan.entry_price) / scan.atr
        } else { 0.0 };
        let max_dd_pct = (scan.entry_price - scan.min_low) / scan.entry_price * 100.0;
        let days = (scan.exit_time - scan.entry_time).num_days();
        let bucket = ScanBucket::classify_3way(pct, days, max_dd_pct, max_runup_atr);

        self.raw_results.push(IvScanResult {
            ticker: scan.ticker,
            entry_time: EtFormat::utc(&scan.entry_time),
            exit_time: EtFormat::utc(&scan.exit_time),
            entry_price: scan.entry_price,
            exit_price: scan.exit_price,
            atr: scan.atr,
            profit_pct: pct,
            max_runup_atr,
            exit_reason: scan.exit_reason.to_string(),
            entry_time_sec: scan.entry_time_sec,
            exit_time_sec: scan.exit_time.timestamp(),
            spike_bars: vec![scan.spike_bar],
            bucket,
            snapshot: scan.snapshot,
            entry_snapshot: scan.entry_snapshot,
        });
    }
}

// ─── Scan-best gate failure analysis ─────────────────────────────────────────

/// Gate failure stats for a single gate.
pub struct GateFailStats {
    pub gate: VfGate,
    pub count: u32,
    pub profit_sum: f64,
    /// Entries where THIS is the only failing gate.
    pub sole_count: u32,
    pub sole_profit_sum: f64,
}

/// For each "best" scan entry, check which VF gates would reject it.
pub fn scan_best_gate_failures(
    results: &[IvScanResult],
    cfg: &StrategyConfig,
) -> Vec<GateFailStats> {
    let best: Vec<_> = results.iter().filter(|r| r.bucket == ScanBucket::Best).collect();
    if best.is_empty() { return vec![]; }

    let mut fails: HashMap<VfGate, (u32, f64, u32, f64)> = HashMap::new();

    for r in &best {
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
            if sole {
                e.2 += 1;
                e.3 += r.profit_pct;
            }
        }
    }

    // Print compact diagnostics for sole-blocked best entries
    for r in &best {
        let g = &r.entry_snapshot.gate;
        let compress = VfCompressParams::new(r.entry_snapshot.iv_compression_ratio, cfg);
        let failed: Vec<VfGate> = VfGate::SCAN_GATES.iter()
            .copied()
            .filter(|&gate| !g.check_gate(gate, cfg, &compress))
            .collect();
        if failed.len() == 1 {
            let tag = failed[0].name();
            match failed[0] {
                VfGate::CwWeak => {
                    eprintln!("  sole_{tag}| +{:>5.1}% gpos={:.2} cw_scw={:.2} ticker={:?}",
                        r.profit_pct, g.gamma_pos,
                        g.cw_vs_scw_atr.unwrap_or(f64::NAN), r.ticker);
                }
                VfGate::AtrPct => {
                    eprintln!("  sole_{tag}| +{:>5.1}% atr_pct={:.3} slow_atr={:.3} tsi={:.1} gpos={:.2} ticker={:?}",
                        r.profit_pct, g.atr_pct, g.slow_atr_pct, g.regime.tsi, g.gamma_pos, r.ticker);
                }
                VfGate::SpreadWide => {
                    eprintln!("  sole_{tag}| +{:>5.1}% spread={:.1} gpos={:.2} tsi={:.1} ticker={:?}",
                        r.profit_pct, g.wall_spread_atr.unwrap_or(f64::NAN), g.gamma_pos, g.regime.tsi, r.ticker);
                }
                VfGate::TsiDead => {
                    eprintln!("  sole_{tag}| +{:>5.1}% tsi={:.1} adx={:.1} atr_ratio={:.2} ticker={:?}",
                        r.profit_pct, g.regime.tsi, g.regime.adx, g.regime.atr_regime_ratio, r.ticker);
                }
                VfGate::IvCompress => {
                    eprintln!("  sole_{tag}| +{:>5.1}% compress={:.3} tsi={:.1} bars_spike={} ticker={:?}",
                        r.profit_pct, r.entry_snapshot.iv_compression_ratio, g.regime.tsi, g.bars_since_spike, r.ticker);
                }
                _ => {
                    eprintln!("  sole_{tag}| +{:>5.1}% ticker={:?}", r.profit_pct, r.ticker);
                }
            }
        }
    }

    let mut out: Vec<_> = fails.into_iter()
        .map(|(gate, (c, p, sc, sp))| GateFailStats {
            gate, count: c, profit_sum: p, sole_count: sc, sole_profit_sum: sp,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count));
    out
}

// ─── CSV export for ML diagnostics ──────────────────────────────────────────

/// Export scan results as CSV for offline ML analysis.
/// Includes both spike-time (sp_*) and entry-time features.
/// Returns number of rows written.
pub fn export_scan_csv(results: &[IvScanResult], path: &std::path::Path) -> anyhow::Result<usize> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    writeln!(f, "ticker,bucket,profit_pct,max_runup_atr,exit_reason,bars_since_spike,\
atr_pct,slow_atr_pct,tsi,adx,atr_regime_ratio,gamma_pos,net_gex,gex_abs_ema,\
pw_vs_spw_atr,cw_vs_scw_atr,wall_spread_atr,\
iv_now,iv_spike_level,iv_compression_ratio,iv_base_ratio,iv_spike_ratio,\
atr_at_spike,atr_spike_ratio,cum_iv_drop,cum_return_atr,spike_mfe_atr,spike_mae_atr,\
pw_dist_atr,cw_dist_atr,spot,\
sp_atr_pct,sp_slow_atr_pct,sp_tsi,sp_adx,sp_atr_regime_ratio,sp_gamma_pos,sp_net_gex,sp_gex_abs_ema,\
sp_pw_vs_spw_atr,sp_cw_vs_scw_atr,sp_wall_spread_atr,sp_pw_dist_atr,sp_cw_dist_atr,\
spike_vanna,spike_gamma_tilt,pw_drift_atr,net_vanna,gamma_tilt")?;

    let mut n = 0usize;
    for r in results {
        let s = &r.entry_snapshot;
        let g = &s.gate;
        let sp = &r.snapshot;
        let sg = &sp.gate;
        writeln!(f, "{:?},{:?},{:.4},{:.4},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},\
{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},\
{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},\
{:.4},{:.4},{:.4},{:.4},{:.4}",
            r.ticker, r.bucket, r.profit_pct, r.max_runup_atr, r.exit_reason, g.bars_since_spike,
            g.atr_pct, g.slow_atr_pct, g.regime.tsi, g.regime.adx, g.regime.atr_regime_ratio,
            g.gamma_pos, g.net_gex, g.gex_abs_ema,
            opt_f64(g.pw_vs_spw_atr), opt_f64(g.cw_vs_scw_atr), opt_f64(g.wall_spread_atr),
            s.iv_now, s.iv_spike_level, s.iv_compression_ratio, s.iv_base_ratio, s.iv_spike_ratio,
            s.atr_at_spike, s.atr_spike_ratio, s.cum_iv_drop, s.gate.cum_return_atr, s.spike_mfe_atr, s.spike_mae_atr,
            opt_f64(s.pw_dist_atr), opt_f64(s.cw_dist_atr), s.spot,
            sg.atr_pct, sg.slow_atr_pct, sg.regime.tsi, sg.regime.adx, sg.regime.atr_regime_ratio,
            sg.gamma_pos, sg.net_gex, sg.gex_abs_ema,
            opt_f64(sg.pw_vs_spw_atr), opt_f64(sg.cw_vs_scw_atr), opt_f64(sg.wall_spread_atr),
            opt_f64(sp.pw_dist_atr), opt_f64(sp.cw_dist_atr),
            g.spike_vanna, g.spike_gamma_tilt, g.pw_drift_atr, g.net_vanna, g.gamma_tilt,
        )?;
        n += 1;
    }
    Ok(n)
}

fn opt_f64(v: Option<f64>) -> String {
    v.map_or_else(String::new, |x| format!("{:.4}", x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_best() {
        // 5% profit, 1 day, 1% dd -> best
        assert_eq!(ScanBucket::classify(5.0, 1, 1.0), ScanBucket::Best);
    }

    #[test]
    fn classify_low_profit_is_worst() {
        assert_eq!(ScanBucket::classify(2.0, 1, 0.5), ScanBucket::Worst);
    }

    #[test]
    fn classify_high_dd_is_worst() {
        assert_eq!(ScanBucket::classify(5.0, 1, 4.0), ScanBucket::Worst);
    }

    #[test]
    fn classify_inefficient_is_worst() {
        // 5% over 20 days = 0.25%/day < 0.5 threshold
        assert_eq!(ScanBucket::classify(5.0, 20, 1.0), ScanBucket::Worst);
    }

    #[test]
    fn classify_zero_days_treated_as_one() {
        assert_eq!(ScanBucket::classify(5.0, 0, 1.0), ScanBucket::Best);
    }

    fn default_gate_ctx() -> VfGateCtx {
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

    fn default_snap() -> ScanSnapshot {
        ScanSnapshot {
            gate: default_gate_ctx(),
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

    fn make_result(ticker: Ticker, profit: f64, bucket: ScanBucket, gate: VfGateCtx) -> IvScanResult {
        let mut snap = default_snap();
        snap.gate = gate;
        IvScanResult {
            ticker,
            entry_time: "2024-03-15T14:30:00Z".into(),
            exit_time: "2024-03-20T15:00:00Z".into(),
            entry_price: 150.0,
            exit_price: 150.0 * (1.0 + profit / 100.0),
            atr: 2.0,
            profit_pct: profit,
            max_runup_atr: 3.0,
            exit_reason: "tp".into(),
            entry_time_sec: 1710510600,
            exit_time_sec: 1710943200,
            spike_bars: vec![100],
            bucket,
            snapshot: default_snap(),
            entry_snapshot: snap,
        }
    }

    #[test]
    fn gate_failures_empty_on_no_best() {
        let cfg = StrategyConfig::default();
        let results = vec![make_result(Ticker::AAPL, 1.0, ScanBucket::Worst, default_gate_ctx())];
        assert!(scan_best_gate_failures(&results, &cfg).is_empty());
    }

    #[test]
    fn gate_failures_empty_when_all_pass() {
        let cfg = StrategyConfig::default();
        let results = vec![make_result(Ticker::AAPL, 5.0, ScanBucket::Best, default_gate_ctx())];
        assert!(scan_best_gate_failures(&results, &cfg).is_empty());
    }

    #[test]
    fn gate_failures_counts_sole() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate_ctx();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0); // fails CwWeak only

        let results = vec![make_result(Ticker::GOOG, 8.0, ScanBucket::Best, gate)];
        let stats = scan_best_gate_failures(&results, &cfg);
        let cw = stats.iter().find(|s| s.gate == VfGate::CwWeak).unwrap();
        assert_eq!(cw.count, 1);
        assert_eq!(cw.sole_count, 1);
        assert!((cw.sole_profit_sum - 8.0).abs() < 0.01);
    }

    #[test]
    fn gate_failures_multi_gate_no_sole() {
        let cfg = StrategyConfig::default();
        let mut gate = default_gate_ctx();
        gate.gamma_pos = 0.0;
        gate.cw_vs_scw_atr = Some(-5.0); // fails CwWeak
        gate.wall_spread_atr = Some(50.0); // fails SpreadWide

        let results = vec![make_result(Ticker::AAPL, 10.0, ScanBucket::Best, gate)];
        let stats = scan_best_gate_failures(&results, &cfg);
        for s in &stats {
            assert_eq!(s.sole_count, 0);
        }
    }

    #[test]
    fn iv_scan_result_serializes_time_fields() {
        let r = make_result(Ticker::AAPL, 5.0, ScanBucket::Best, default_gate_ctx());
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["entryTimeSec"], 1710510600);
        assert_eq!(json["exitTimeSec"], 1710943200);
    }
}
