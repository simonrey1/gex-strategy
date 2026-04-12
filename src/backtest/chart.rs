use crate::config::{BarIndex, strategy::IV_LOOKBACK_BARS};

use crate::types::{GexProfile, OhlcBar, Rejection, Signal};

use super::positions::Trade;
use super::state::{ChartBar, Marker, SpikeWindow, WallBand, WallBandBar, WallPoint};
use super::types::{ChartExitRecord, RecordTradeInputs, TickerState};

// ─── Chart colors (single source of truth for Rust-side markers) ────────────

const COLOR_VF: &str = "#f57c00";
const COLOR_WB: &str = "#4caf50";
const COLOR_WIN: &str = "#26a69a";
const COLOR_LOSS: &str = "#ef5350";
const COLOR_IV_SPIKE: &str = "#ff6d00";
const COLOR_SCAN_BEST: &str = "#00c853";
const COLOR_SCAN_WORST: &str = "#ff1744";
const COLOR_SCAN_EXIT: &str = "#00bcd4";

// ─── Chart marker helpers ───────────────────────────────────────────────────

fn signal_marker_prefix(signal: Signal) -> &'static str {
    signal.short_name()
}

fn fmt_pct1(pct: f64) -> String {
    if pct >= 0.0 { format!("+{:.1}%", pct) } else { format!("{:.1}%", pct) }
}

fn signal_marker_color(signal: Signal) -> &'static str {
    match signal {
        Signal::LongVannaFlip => COLOR_VF,
        Signal::LongWallBounce => COLOR_WB,
        Signal::Flat => "#888888",
    }
}

fn is_entry_marker(m: &Marker) -> bool {
    m.shape == "arrowUp"
        && (m.text.starts_with(Signal::LongVannaFlip.short_name())
            || m.text.starts_with(Signal::LongWallBounce.short_name()))
}

/// Calendar days between two RFC3339 or "YYYY-MM-DD …" timestamps.
fn trade_days(entry: &str, exit: &str) -> i64 {
    let parse = |s: &str| {
        let date_str = s.get(..10)?;
        chrono::NaiveDate::parse_from_str(date_str, crate::types::DATE_FMT).ok()
    };
    match (parse(entry), parse(exit)) {
        (Some(e), Some(x)) => (x - e).num_days(),
        _ => 0,
    }
}

pub struct ScanEntryMarkerInputs<'a> {
    pub entry_sec: i64,
    pub exit_sec: i64,
    pub pct: f64,
    pub max_runup_atr: f64,
    pub exit_time: &'a str,
    pub bucket: super::iv_scan::ScanBucket,
}

fn scan_days(entry_sec: i64, exit_sec: i64) -> i64 {
    use chrono::TimeZone;
    let e = chrono::Utc.timestamp_opt(entry_sec, 0).single();
    let x = chrono::Utc.timestamp_opt(exit_sec, 0).single();
    match (e, x) {
        (Some(e), Some(x)) => (x.date_naive() - e.date_naive()).num_days(),
        _ => 0,
    }
}

impl Marker {
    pub fn entry(time: i64, signal: Signal) -> Self {
        Self {
            time,
            position: "belowBar".to_string(),
            color: signal_marker_color(signal).to_string(),
            shape: "arrowUp".to_string(),
            text: signal_marker_prefix(signal).to_string(),
            size: Some(2),
        }
    }

    pub fn exit(time: i64, trade: &Trade) -> Self {
        let color = if trade.net_pnl >= 0.0 { COLOR_WIN } else { COLOR_LOSS };
        Self {
            time,
            position: "aboveBar".to_string(),
            color: color.to_string(),
            shape: "arrowDown".to_string(),
            text: fmt_pct1(trade.return_pct),
            size: Some(2),
        }
    }

    fn iv_spike(time: i64, iv_peak: f64) -> Self {
        Self {
            time,
            position: "belowBar".to_string(),
            color: COLOR_IV_SPIKE.to_string(),
            shape: "square".to_string(),
            text: format!("IV↑{:.2}", iv_peak),
            size: Some(1),
        }
    }

    /// IV-scan entry marker. Shape/color/position driven by bucket classification.
    pub fn scan_entry(i: ScanEntryMarkerInputs<'_>) -> Self {
        use super::iv_scan::ScanBucket::*;
        let pct_s = fmt_pct1(i.pct);
        let exit_date = if i.exit_time.len() >= 10 { &i.exit_time[..10] } else { i.exit_time };
        let days = scan_days(i.entry_sec, i.exit_sec);
        let peak = format!("peak {:.0}atr", i.max_runup_atr);
        let (position, color, shape, text) = match i.bucket {
            Best   => ("belowBar", COLOR_SCAN_BEST,  "arrowUp",  format!("Scan▲{} {} → {} ({}d)", pct_s, peak, exit_date, days)),
            Middle => ("aboveBar", "#ffa726",         "diamond",  format!("~{} {} → {} ({}d)", pct_s, peak, exit_date, days)),
            Worst  => ("aboveBar", COLOR_SCAN_WORST, "square",   format!("✕{} {} → {} ({}d)", pct_s, peak, exit_date, days)),
        };
        Self { time: i.entry_sec, position: position.into(), color: color.into(), shape: shape.into(), text, size: Some(1) }
    }

    /// IV-scan exit marker (only emitted for Best trades).
    pub fn scan_exit(time: i64, pct: f64) -> Self {
        Self {
            time,
            position: "aboveBar".into(),
            color: COLOR_SCAN_EXIT.into(),
            shape: "arrowDown".into(),
            text: format!("Scan▼+{:.1}%", pct),
            size: Some(1),
        }
    }
}

/// All derived values for a single bar during an active spike.
/// Computed once, used by tooltip formatting and potentially by strategy diagnostics.
pub struct SpikeBarDiag {
    pub tsi: f64,
    pub adx: f64,
    pub bars_since_spike: BarIndex,
    pub atr_pct: f64,
    pub atr_regime_ratio: f64,
    pub iv: f64,
    pub spike_level: f64,
    pub compress_pct: f64,
    pub gex_norm: f64,
    pub net_gex: f64,
    pub wall_diff: crate::strategy::shared::WallAtrDiffs,
    pub has_walls: bool,
    pub spread_pw: f64,
    pub spread_cw: f64,
    pub spread_max: f64,
    pub has_spread: bool,
}

impl SpikeBarDiag {
    pub fn compute(bctx: &crate::strategy::entries::BarCtx) -> Self {
        let atr = bctx.ind.atr;
        let iv = bctx.atm_put_iv_or_zero();
        let (atr_pct, _) = bctx.vf_atr_pct_pair();
        let s = bctx.state;

        Self {
            tsi: bctx.ind.tsi,
            adx: bctx.ind.adx,
            bars_since_spike: s.bars_since_spike(),
            atr_pct,
            atr_regime_ratio: bctx.ind.atr_regime_ratio,
            iv,
            spike_level: s.iv_spike_level,
            compress_pct: s.compress_pct(iv),
            gex_norm: s.gex_norm(bctx.net_gex()),
            net_gex: bctx.net_gex(),
            wall_diff: bctx.wall_diff_atr(),
            has_walls: s.has_valid_walls(atr),
            spread_pw: s.spread_put_wall(),
            spread_cw: s.spread_call_wall(),
            spread_max: bctx.cfg.vf_max_wall_spread_atr,
            has_spread: s.has_valid_spread(atr),
        }
    }

    pub fn format_lines(&self, reasons: &[Rejection]) -> Vec<String> {
        let mut lines = Vec::with_capacity(8 + reasons.len());
        if reasons.is_empty() {
            lines.push("✓ All gates pass".into());
        } else {
            for r in reasons {
                lines.push(format!("✗ {r}"));
            }
        }
        lines.push(format!("TSI: {:.1}  ADX: {:.1}  Bars: {}", self.tsi, self.adx, self.bars_since_spike));
        lines.push(format!("ATR%: {:.3}  Regime: {:.2}", self.atr_pct, self.atr_regime_ratio));
        lines.push(format!("IV: {:.4}  Spike: {:.4}  Comp: {:.0}%", self.iv, self.spike_level, self.compress_pct));
        lines.push(format!("GEX norm: {:.2}  Net GEX: {:.0}", self.gex_norm, self.net_gex));
        if self.has_walls {
            lines.push(format!(
                "PW−SPW: {:.2}  CW−SCW: {:.2}",
                self.wall_diff.pw_spw_atr, self.wall_diff.cw_scw_atr,
            ));
        }
        if self.has_spread {
            lines.push(format!(
                "Spread: {:.1}atr (${:.0}–${:.0})  max: {:.1}",
                self.wall_diff.spread_atr, self.spread_pw, self.spread_cw, self.spread_max,
            ));
        }
        lines
    }
}

// ─── Chart data collection ──────────────────────────────────────────────────

fn snap_to_bar(time: i64) -> i64 {
    let bucket = crate::config::bar_interval::bucket_secs_i64(crate::config::BAR_INTERVAL_MINUTES);
    time - (time % bucket)
}

impl TickerState {
    pub fn push_entry_marker(&mut self, bar_time_sec: i64, signal: crate::types::Signal) {
        if !self.save_chart { return; }
        self.chart_data.markers.push(Marker::entry(snap_to_bar(bar_time_sec), signal));
    }

    pub fn push_exit_marker(&mut self, bar_time_sec: i64, trade: &Trade) {
        if !self.save_chart { return; }
        self.chart_data.markers.push(Marker::exit(snap_to_bar(bar_time_sec), trade));
        if let Some(m) = self.chart_data.markers.iter_mut().rev()
            .find(|m| is_entry_marker(m))
        {
            let pct_s = fmt_pct1(trade.return_pct);
            let exit_date = &trade.exit_time[..10];
            let days = trade_days(&trade.entry_time, &trade.exit_time);
            m.text = format!("{} → {} {} ({}d)", m.text, exit_date, pct_s, days);
        }
    }

    /// Common exit bookkeeping: marker, engine state update, trade recording, position clearing.
    /// Returns the trade's net PnL (caller still needs to update portfolio cash and open_positions).
    pub fn record_exit(&mut self, i: ChartExitRecord) {
        self.push_exit_marker(i.bar_time_sec, &i.trade);
        self.engine
            .on_exit(&mut self.daily, i.trade.net_pnl, i.equity);
        self.record_trade(RecordTradeInputs { trade: i.trade, bar_time_sec: i.bar_time_sec, starting_capital: i.starting_capital });
        self.position = None;
    }

    /// Push an OHLC bar to the chart series (normalized to post-split-adjusted prices).
    pub fn push_bar(&mut self, bar: &OhlcBar, bar_time_sec: i64) {
        let r = self.chart_split_ratio;
        self.chart_data.bars.push(ChartBar {
            time: bar_time_sec,
            open: bar.open / r,
            high: bar.high / r,
            low: bar.low / r,
            close: bar.close / r,
        });
    }

    /// Push GEX wall levels (put/call layer 1 + wide walls + smoothed),
    /// normalized to post-split-adjusted prices.
    pub fn collect_wall_chart_data(&mut self, gex: &GexProfile, bar_time_sec: i64) {
        let r = self.chart_split_ratio;
        let wp = |strike: f64| WallPoint { time: bar_time_sec, value: strike / r };
        let pw_bands = [&mut self.chart_data.put_walls, &mut self.chart_data.put_wall_2,
                        &mut self.chart_data.put_wall_3, &mut self.chart_data.put_wall_4,
                        &mut self.chart_data.put_wall_5];
        for (i, dest) in pw_bands.into_iter().enumerate() {
            if let Some(w) = gex.put_walls.get(i) { dest.push(wp(w.strike)); }
        }
        let cw_bands = [&mut self.chart_data.call_walls, &mut self.chart_data.call_wall_2,
                        &mut self.chart_data.call_wall_3, &mut self.chart_data.call_wall_4,
                        &mut self.chart_data.call_wall_5];
        for (i, dest) in cw_bands.into_iter().enumerate() {
            if let Some(w) = gex.call_walls.get(i) { dest.push(wp(w.strike)); }
        }
        // Wall bands with proportional gamma concentration
        let pw_total: f64 = gex.put_walls.iter().map(|w| w.gamma_oi.abs()).sum();
        let cw_total: f64 = gex.call_walls.iter().map(|w| w.gamma_oi.abs()).sum();
        let to_band = |walls: &[crate::types::WallLevel], total: f64| -> Vec<WallBand> {
            walls.iter().take(5).map(|w| WallBand {
                strike: w.strike / r,
                pct: if total > 0.0 { w.gamma_oi.abs() / total } else { 0.0 },
            }).collect()
        };
        self.chart_data.wall_bands.push(WallBandBar {
            time: bar_time_sec,
            put_walls: to_band(&gex.put_walls, pw_total),
            call_walls: to_band(&gex.call_walls, cw_total),
        });
        if let Some(w) = gex.wide_put_walls.first() {
            self.chart_data.wide_put_wall.push(wp(w.strike));
        }
        if let Some(w) = gex.wide_call_walls.first() {
            self.chart_data.wide_call_wall.push(wp(w.strike));
        }
        if self.wall_smoother.is_enabled() {
            let spw = self.wall_smoother.pw();
            let scw = self.wall_smoother.cw();
            if spw > 0.0 { self.chart_data.smooth_put_wall.push(wp(spw)); }
            if scw > 0.0 { self.chart_data.smooth_call_wall.push(wp(scw)); }
            let shpw = self.wall_smoother.smoothed_highest_pw();
            let slcw = self.wall_smoother.smoothed_lowest_cw();
            if shpw > 0.0 { self.chart_data.smooth_highest_pw.push(wp(shpw)); }
            if slcw > 0.0 { self.chart_data.smooth_lowest_cw.push(wp(slcw)); }
            let spread_pw = self.wall_smoother.spread_pw();
            let spread_cw = self.wall_smoother.spread_cw();
            if spread_pw > 0.0 { self.chart_data.spread_put_wall.push(wp(spread_pw)); }
            if spread_cw > 0.0 { self.chart_data.spread_call_wall.push(wp(spread_cw)); }
        }
    }

    /// Emit IV fast/slow EMA series and crossover markers (EOD only to avoid noise).
    pub fn collect_iv_ema_chart(&mut self, bar_time_sec: i64, is_eod: bool) {
        if !self.save_chart { return; }
        let ss = &self.engine.signal_state;
        if ss.iv_ema_fast == 0.0 { return; }
        let wp = |v: f64| WallPoint { time: bar_time_sec, value: v };
        self.chart_data.iv_ema_fast.push(wp(ss.iv_ema_fast));
        self.chart_data.iv_ema_slow.push(wp(ss.iv_ema_slow));
        if !is_eod { return; }
        let dir = ss.iv_cross_dir;
        if self.prev_iv_cross_dir != 0 && dir != 0 && dir != self.prev_iv_cross_dir {
            let (color, shape, label) = if dir < 0 {
                ("#26a69a", "arrowUp", "IV↓")
            } else {
                ("#ef5350", "arrowDown", "IV↑")
            };
            self.chart_data.markers.push(Marker {
                time: bar_time_sec,
                position: "belowBar".into(),
                color: color.into(),
                shape: shape.into(),
                text: label.into(),
                size: Some(1),
            });
        }
        self.prev_iv_cross_dir = dir;
    }

    /// Emit spike marker when a new spike appears; open a spike window (end=0).
    pub fn collect_iv_markers(&mut self, bar_time_sec: i64) {
        let signal_state = &self.engine.signal_state;
        let cur_spike_bar = signal_state.iv_spike_bar;
        if signal_state.has_active_spike() && cur_spike_bar != self.last_iv_spike_bar_marked {
            self.last_iv_spike_bar_marked = cur_spike_bar;
            self.chart_data.markers.push(Marker::iv_spike(bar_time_sec, signal_state.iv_spike_level));
            self.chart_data.spike_windows.push(SpikeWindow { start: bar_time_sec, end: 0 });
        }
    }

    /// Close the open spike window when the spike expires or IV_LOOKBACK_BARS elapsed.
    pub fn close_spike_window_if_expired(&mut self, bar_time_sec: i64) {
        let ss = &self.engine.signal_state;
        if let Some(w) = self.chart_data.spike_windows.last_mut() {
            if w.end == 0 {
                let expired = !ss.has_active_spike()
                    || ss.bars_since_spike() >= IV_LOOKBACK_BARS as BarIndex;
                if expired {
                    w.end = bar_time_sec;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_pct1_positive() {
        assert_eq!(fmt_pct1(5.12), "+5.1%");
    }

    #[test]
    fn fmt_pct1_negative() {
        assert_eq!(fmt_pct1(-3.78), "-3.8%");
    }

    #[test]
    fn fmt_pct1_zero() {
        assert_eq!(fmt_pct1(0.0), "+0.0%");
    }

    #[test]
    fn signal_marker_prefix_vf() {
        assert_eq!(signal_marker_prefix(Signal::LongVannaFlip), "VF");
    }

    #[test]
    fn signal_marker_prefix_wb() {
        assert_eq!(signal_marker_prefix(Signal::LongWallBounce), "WB");
    }

    #[test]
    fn trade_days_same_day() {
        assert_eq!(trade_days("2025-01-15 10:00", "2025-01-15 15:00"), 0);
    }

    #[test]
    fn trade_days_multi() {
        assert_eq!(trade_days("2025-01-10", "2025-01-13"), 3);
    }

    #[test]
    fn trade_days_bad_input() {
        assert_eq!(trade_days("bad", "data"), 0);
    }

    #[test]
    fn snap_to_bar_rounds_down() {
        let bucket = crate::config::bar_interval::bucket_secs_i64(crate::config::BAR_INTERVAL_MINUTES);
        let t = bucket * 3 + 120;
        assert_eq!(snap_to_bar(t), bucket * 3);
    }

    #[test]
    fn entry_marker_fields() {
        let m = Marker::entry(1000, Signal::LongVannaFlip);
        assert_eq!(m.shape, "arrowUp");
        assert_eq!(m.text, "VF");
        assert_eq!(m.time, 1000);
    }

    #[test]
    fn exit_marker_profit() {
        let trade = Trade {
            ticker: crate::config::Ticker::AAPL,
            signal: Signal::LongVannaFlip,
            entry_time: "2025-01-10T10:00:00Z".into(),
            exit_time: "2025-01-11T10:00:00Z".into(),
            entry_price: 100.0,
            exit_price: 110.0,
            shares: 10,
            gross_pnl: 100.0,
            net_pnl: 95.0,
            return_pct: 10.0,
            commission: 2.0,
            slippage: 3.0,
            exit_reason: "take_profit".into(),
            bars_held: 1,
            spike_bar: 0,
            max_runup_atr: 5.0,
            diagnostics: None,
        };
        let m = Marker::exit(2000, &trade);
        assert_eq!(m.color, COLOR_WIN);
        assert!(m.text.contains("+10.0%"));
    }

    #[test]
    fn is_entry_marker_true() {
        let m = Marker::entry(100, Signal::LongVannaFlip);
        assert!(is_entry_marker(&m));
    }

    fn make_ticker_state() -> TickerState {
        let cfg = crate::config::StrategyConfig::default();
        let mut ts = TickerState::new(&cfg);
        ts.save_chart = true;
        ts
    }

    #[test]
    fn collect_iv_markers_emits_spike_window() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.collect_iv_markers(1000);
        assert_eq!(ts.chart_data.spike_windows.len(), 1);
        assert_eq!(ts.chart_data.spike_windows[0].start, 1000);
        assert_eq!(ts.chart_data.spike_windows[0].end, 0);
    }

    #[test]
    fn collect_iv_markers_no_duplicate_same_spike() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.collect_iv_markers(1000);
        ts.collect_iv_markers(2000);
        assert_eq!(ts.chart_data.spike_windows.len(), 1);
        assert_eq!(ts.chart_data.markers.len(), 1);
    }

    #[test]
    fn collect_iv_markers_new_spike_new_window() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.collect_iv_markers(1000);

        ts.engine.signal_state.iv_spike_bar = 20;
        ts.collect_iv_markers(2000);
        assert_eq!(ts.chart_data.spike_windows.len(), 2);
        assert_eq!(ts.chart_data.spike_windows[1].start, 2000);
    }

    #[test]
    fn close_spike_window_when_expired() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.engine.signal_state.bar_index = 10;
        ts.collect_iv_markers(1000);

        ts.engine.signal_state.bar_index = 10 + IV_LOOKBACK_BARS as BarIndex;
        ts.close_spike_window_if_expired(5000);
        assert_eq!(ts.chart_data.spike_windows[0].end, 5000);
    }

    #[test]
    fn close_spike_window_not_yet_expired() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.engine.signal_state.bar_index = 10;
        ts.collect_iv_markers(1000);

        ts.engine.signal_state.bar_index = 10 + IV_LOOKBACK_BARS as BarIndex - 1;
        ts.close_spike_window_if_expired(3000);
        assert_eq!(ts.chart_data.spike_windows[0].end, 0);
    }

    #[test]
    fn close_spike_window_when_spike_cleared() {
        let mut ts = make_ticker_state();
        ts.engine.signal_state.iv_spike_level = 0.50;
        ts.engine.signal_state.iv_spike_bar = 10;
        ts.engine.signal_state.bar_index = 10;
        ts.collect_iv_markers(1000);

        ts.engine.signal_state.iv_spike_level = 0.0;
        ts.engine.signal_state.iv_spike_bar = -1;
        ts.close_spike_window_if_expired(4000);
        assert_eq!(ts.chart_data.spike_windows[0].end, 4000);
    }

    #[test]
    fn is_entry_marker_false_exit() {
        let m = Marker {
            time: 100, position: "aboveBar".into(),
            color: COLOR_WIN.into(), shape: "arrowDown".into(),
            text: "+5%".into(), size: Some(2),
        };
        assert!(!is_entry_marker(&m));
    }
}
