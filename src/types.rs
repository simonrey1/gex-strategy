use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};
use ts_rs::TS;

use crate::config::Ticker;

/// Lock a Mutex, recovering from poison (prior panic while holding).
/// Panicking on a poisoned mutex cascades into process-wide failure;
/// recovering the inner data lets the system continue operating.
#[inline]
pub fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| {
        eprintln!("[WARN] Mutex poisoned — recovering: {}", e);
        e.into_inner()
    })
}

/// Standard date format used throughout the codebase for YYYY-MM-DD strings.
pub const DATE_FMT: &str = "%Y-%m-%d";

// ─── Numeric conversions (traits — prefer `.to_f64()` / `.trunc_u64()` over ad-hoc `as`) ───

/// Widen counts to `f64` (lengths, shares, ticks, `i64` minutes / OI).
pub trait ToF64: Copy {
    fn to_f64(self) -> f64;
}
impl ToF64 for usize {
    #[inline]
    fn to_f64(self) -> f64 {
        self as f64
    }
}
impl ToF64 for u32 {
    #[inline]
    fn to_f64(self) -> f64 {
        self as f64
    }
}
impl ToF64 for u64 {
    #[inline]
    fn to_f64(self) -> f64 {
        self as f64
    }
}
impl ToF64 for i64 {
    #[inline]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

/// Truncate `f64` for display, portfolio $, backoff ms, order sizes.
pub trait F64Trunc {
    fn trunc_u32(self) -> u32;
    fn trunc_u64(self) -> u64;
}
impl F64Trunc for f64 {
    #[inline]
    fn trunc_u32(self) -> u32 {
        self as u32
    }
    #[inline]
    fn trunc_u64(self) -> u64 {
        self as u64
    }
}

/// Saturating `usize` → `u32` for TS/JSON lengths and small counts.
pub trait AsLenU32 {
    fn as_len_u32(self) -> u32;
}
impl AsLenU32 for usize {
    #[inline]
    fn as_len_u32(self) -> u32 {
        u32::try_from(self).unwrap_or(u32::MAX)
    }
}

/// `chrono` millis (`i64`) → `u64` (negative → 0).
pub trait MillisAsU64 {
    fn millis_as_u64(self) -> u64;
}
impl MillisAsU64 for i64 {
    #[inline]
    fn millis_as_u64(self) -> u64 {
        u64::try_from(self).unwrap_or(0)
    }
}

/// `DateTime` → epoch millis as `u64`.
#[inline]
pub fn datetime_millis_u64(dt: &DateTime<Utc>) -> u64 {
    dt.timestamp_millis().millis_as_u64()
}

/// Decode a strike aggregation key ([`strike_key`]) back to dollars.
#[inline]
pub fn strike_key_to_f64(key: i64) -> f64 {
    key.to_f64() / 100.0
}

/// Current UTC time as milliseconds since epoch.
#[inline]
pub fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().millis_as_u64()
}

/// `num / den` when `den > 0`, else `default`.
#[inline]
pub fn safe_ratio(num: f64, den: f64, default: f64) -> f64 {
    if den > 0.0 { num / den } else { default }
}

/// `Some(v)` unless `v` is NaN.
#[inline]
pub fn opt_finite(v: f64) -> Option<f64> {
    if v.is_nan() { None } else { Some(v) }
}

// ─── Shared constants ────────────────────────────────────────────────────────

pub const BARS_PER_DAY: usize = 390;

/// IV below this is effectively zero (deep OTM / no market).
pub const MIN_IV: f64 = 0.01;
/// IV above this is a ThetaData sentinel or garbage. Real panic IVs on
/// large-cap US equities peak around 300-500%; 5.0 (500%) is generous.
pub const MAX_IV: f64 = 5.0;
/// Max strike distance from spot for wide-chain caching/processing.
/// Actual structural walls are within ±19% of spot (p99 < 15%).
/// 25% gives comfortable margin while dropping deep OTM garbage.
pub const MAX_STRIKE_DIST_PCT: f64 = 0.25;

/// Bucket a strike price into an integer key for aggregation (cents precision).
#[inline]
pub fn strike_key(strike: f64) -> i64 {
    (strike * 100.0) as i64
}

/// Round a price to the nearest cent (tick size for US equities).
#[inline]
pub fn round_cents(price: f64) -> f64 {
    (price * 100.0).round() / 100.0
}

/// Returns true if the option row has valid data for GEX / wall computation.
/// Shared by all backtest and live ingestion paths — single source of truth.
/// Logs a warning on the first sentinel/garbage IV per run so data issues are visible.
#[inline]
pub fn option_row_valid(underlying_price: f64, gamma: f64, iv: f64, oi: f64, strike: f64) -> bool {
    if iv > MAX_IV {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "[WARN] option row rejected: iv={:.4} (>{MAX_IV}) spot={:.2} gamma={:.6} oi={:.0} — ThetaData sentinel or deep OTM garbage",
                iv, underlying_price, gamma, oi,
            );
        }
        return false;
    }
    underlying_price > 0.0
        && gamma > 0.0
        && iv > MIN_IV
        && oi > 0.0
        && (strike - underlying_price).abs() / underlying_price <= MAX_STRIKE_DIST_PCT
}

/// NaN-safe f64 comparison. NaN is treated as -∞ (smallest), so NaN values
/// sort last in descending order and are never selected by `max_by`.
#[inline]
pub fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or_else(|| match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, _) => std::cmp::Ordering::Less,
        (_, true) => std::cmp::Ordering::Greater,
        _ => unreachable!(),
    })
}

pub fn fmt_pct(pct: f64) -> String {
    if pct >= 0.0 {
        format!("+{:.2}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

// ─── Market data ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OhlcBar {
    pub timestamp: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl OhlcBar {
    /// Last 15-min bar of the US trading day (15:45 ET = 19:45 or 20:45 UTC).
    pub fn is_eod(&self) -> bool {
        let m = self.timestamp.minute();
        let h = self.timestamp.hour();
        m == 45 && (h == 19 || h == 20)
    }
}

// ─── Options ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OptionContract {
    pub symbol: String,
    pub strike: f64,
    pub expiry: DateTime<Utc>,
    pub is_call: bool,
    pub oi: f64,
    pub gamma: f64,
    pub iv: f64,
    pub vanna: f64,
    pub delta: f64,
    pub vega: f64,
}

#[derive(Debug, Clone)]
pub struct OptionsSnapshot {
    pub timestamp: DateTime<Utc>,
    pub underlying: Ticker,
    pub spot: f64,
    pub contracts: Vec<OptionContract>,
}

// ─── GEX ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WallLevel {
    pub strike: f64,
    #[serde(rename = "gammaOI")]
    pub gamma_oi: f64,
}

/// Σ γ×OI for a wall slice (narrow / wide / trail lists).
#[inline]
pub fn wall_gamma_oi_sum(walls: &[WallLevel]) -> f64 {
    walls.iter().map(|w| w.gamma_oi).sum()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GexProfile {
    pub spot: f64,
    #[serde(rename = "netGex")]
    pub net_gex: f64,
    #[serde(rename = "putWalls")]
    pub put_walls: Vec<WallLevel>,
    #[serde(rename = "callWalls")]
    pub call_walls: Vec<WallLevel>,
    /// Average IV of puts within ±5% of spot, from unfiltered 15-min snapshots.
    /// Includes near-expiry (0DTE/weekly) contracts that spike violently when
    /// puts go ITM — the key signal in the MenthorQ vanna flip article.
    /// High = IV spike (puts ITM, dealers selling). Low after spike = compression entry.
    #[serde(rename = "atmPutIv", default, skip_serializing_if = "Option::is_none")]
    pub atm_put_iv: Option<f64>,
    /// Structural put walls from wide-strike daily snapshot (±25% OTM).
    /// Empty when wide data unavailable. Top 5, strongest first.
    /// Used for VannaFlip SL anchoring and entry spread filter.
    #[serde(rename = "widePutWalls", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_put_walls: Vec<WallLevel>,
    /// Structural call walls from wide-strike daily snapshot (±25% OTM).
    /// Empty when wide data unavailable. Top 5, strongest first.
    /// Used for VannaFlip TP targeting and entry spread filter.
    #[serde(rename = "wideCallWalls", default, skip_serializing_if = "Vec::is_empty")]
    pub wide_call_walls: Vec<WallLevel>,
    // ── Aggregate gamma features (from full options chain) ──
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

    /// Fraction of total call gamma that sits ABOVE the narrow CW strike.
    /// High = deep backup gamma behind the CW (structural). Low = CW is the frontier.
    #[serde(rename = "cwDepthRatio", default)]
    pub cw_depth_ratio: f64,

    /// Normalized dealer gamma regime: (call_goi − put_goi) / (call_goi + put_goi).
    /// Range [-1, 1]. Positive = call gamma dominates (dealers dampen moves, walls are barriers).
    /// Negative = put gamma dominates (dealers amplify moves, CW is a magnet).
    #[serde(rename = "gammaTilt", default)]
    pub gamma_tilt: f64,

    // ── Vanna / delta aggregates ──
    /// Net vanna exposure: Σ(vanna × OI × 100) across all contracts.
    /// Positive = IV drop causes dealers to buy stock (bullish flow).
    #[serde(rename = "netVanna", default)]
    pub net_vanna: f64,
    /// Net delta exposure: Σ(delta × OI × 100) across all contracts (signed by side).
    #[serde(rename = "netDelta", default)]
    pub net_delta: f64,
}

/// Bar close and ATR — shared by wall ratchet, trail wall pickers, etc.
#[derive(Debug, Clone, Copy)]
pub struct BarPriceAtr {
    pub close: f64,
    pub atr: f64,
}

impl BarPriceAtr {
    #[inline]
    pub const fn new(close: f64, atr: f64) -> Self {
        Self { close, atr }
    }
}

/// Close + ATR + ATR/EMA(ATR) regime — adaptive thresholds, wall-trail checks, IV scan.
#[derive(Debug, Clone, Copy)]
pub struct BarVolRegime {
    pub close: f64,
    pub atr: f64,
    pub atr_regime_ratio: f64,
}

impl BarVolRegime {
    #[inline]
    pub const fn new(close: f64, atr: f64, atr_regime_ratio: f64) -> Self {
        Self {
            close,
            atr,
            atr_regime_ratio,
        }
    }

    /// ATR normalized by regime ratio (undoes the vol expansion).
    #[inline]
    pub fn slow_atr(self) -> f64 {
        if self.atr_regime_ratio > 0.0 {
            self.atr / self.atr_regime_ratio
        } else {
            self.atr
        }
    }

    #[inline]
    pub const fn price_atr(self) -> BarPriceAtr {
        BarPriceAtr::new(self.close, self.atr)
    }
}

/// ATR + TSI at signal bar (before regime ratio is fixed at execution).
#[derive(Debug, Clone, Copy)]
pub struct EntryAtrTsi {
    pub atr: f64,
    pub tsi: f64,
}

impl EntryAtrTsi {
    #[inline]
    pub const fn new(atr: f64, tsi: f64) -> Self {
        Self { atr, tsi }
    }

    #[inline]
    pub const fn with_atr_regime_ratio(self, atr_regime_ratio: f64) -> AtrRegimeTsi {
        AtrRegimeTsi::new(self.atr, atr_regime_ratio, self.tsi)
    }
}

/// ATR, ATR regime ratio, and TSI — slot sizing, stops, deferred fills.
#[derive(Debug, Clone, Copy)]
pub struct AtrRegimeTsi {
    pub atr: f64,
    pub atr_regime_ratio: f64,
    pub tsi: f64,
}

impl AtrRegimeTsi {
    #[inline]
    pub const fn new(atr: f64, atr_regime_ratio: f64, tsi: f64) -> Self {
        Self {
            atr,
            atr_regime_ratio,
            tsi,
        }
    }

    #[inline]
    pub const fn to_atr_tsi(self) -> EntryAtrTsi {
        EntryAtrTsi::new(self.atr, self.tsi)
    }
}

impl From<&crate::strategy::indicators::IndicatorValues> for AtrRegimeTsi {
    #[inline]
    fn from(ind: &crate::strategy::indicators::IndicatorValues) -> Self {
        Self::new(ind.atr, ind.atr_regime_ratio, ind.tsi)
    }
}

impl GexProfile {
    #[inline] pub fn pw(&self) -> f64 { self.put_walls.first().map(|w| w.strike).unwrap_or(0.0) }
    #[inline] pub fn cw(&self) -> f64 { self.call_walls.first().map(|w| w.strike).unwrap_or(0.0) }
    /// Narrow call wall and narrow put wall strikes (`cw`, `pw`).
    #[inline]
    pub fn narrow_cw_pw(&self) -> (f64, f64) {
        (self.cw(), self.pw())
    }
    #[inline] pub fn pw_opt(&self) -> Option<f64> { self.put_walls.first().map(|w| w.strike) }
    #[inline] pub fn cw_opt(&self) -> Option<f64> { self.call_walls.first().map(|w| w.strike) }

    /// (spot − narrow PW) / ATR — distance above put wall in ATR units.
    #[inline]
    pub fn pw_dist_atr_opt(&self, atr: f64) -> Option<f64> {
        if atr <= 0.0 { return None; }
        self.pw_opt().map(|p| (self.spot - p) / atr)
    }

    /// (narrow CW − spot) / ATR — distance below call wall in ATR units.
    #[inline]
    pub fn cw_dist_atr_opt(&self, atr: f64) -> Option<f64> {
        if atr <= 0.0 { return None; }
        self.cw_opt().map(|c| (c - self.spot) / atr)
    }

    /// Narrow PW dist, narrow CW dist, and their sum (VF wall spread) in ATR units.
    #[inline]
    pub fn narrow_wall_atr_dists(&self, atr: f64) -> (Option<f64>, Option<f64>, Option<f64>) {
        let pw = self.pw_dist_atr_opt(atr);
        let cw = self.cw_dist_atr_opt(atr);
        let spread = match (pw, cw) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        };
        (pw, cw, spread)
    }
    /// Highest strike among the top-2 narrow put walls (by γ×OI).
    #[inline] pub fn top2_highest_pw(&self) -> f64 { self.put_walls.iter().take(2).map(|w| w.strike).fold(0.0_f64, f64::max) }
    /// Lowest strike among the top-2 narrow call walls (by γ×OI).
    #[inline] pub fn top2_lowest_cw(&self) -> f64 { self.call_walls.iter().take(2).map(|w| w.strike).reduce(f64::min).unwrap_or(0.0) }
    #[inline] pub fn wide_pw(&self) -> f64 { self.wide_put_walls.first().map(|w| w.strike).unwrap_or(0.0) }
    #[inline] pub fn wide_cw(&self) -> f64 { self.wide_call_walls.first().map(|w| w.strike).unwrap_or(0.0) }

    /// ATM put IV for bar logic when missing data is treated as zero.
    #[inline]
    pub fn atm_put_iv_or_zero(&self) -> f64 {
        self.atm_put_iv.unwrap_or(0.0)
    }

    /// ATR as percentage of spot price.
    #[inline]
    pub fn atr_pct(&self, atr: f64) -> f64 {
        if self.spot > 0.0 { atr / self.spot * 100.0 } else { 0.0 }
    }

    /// VF gate pair: current-bar ATR% and slow regime ATR% (`VfGateCtx` in `strategy::entries::vf_gates`).
    #[inline]
    pub fn vf_atr_pct_pair(&self, atr: f64, atr_regime_ema: f64) -> (f64, f64) {
        (self.atr_pct(atr), self.atr_pct(atr_regime_ema))
    }

    /// Top put wall (strike, γ×OI). Returns (0, 0) if empty.
    #[inline]
    pub fn pw_top(&self) -> (f64, f64) {
        self.put_walls.first().map(|w| (w.strike, w.gamma_oi)).unwrap_or((0.0, 0.0))
    }

    /// Top call wall (strike, γ×OI). Returns (0, 0) if empty.
    #[inline]
    pub fn cw_top(&self) -> (f64, f64) {
        self.call_walls.first().map(|w| (w.strike, w.gamma_oi)).unwrap_or((0.0, 0.0))
    }

    /// Smoother inputs + concentration for put/call walls.
    /// Returns ((pw_strike, pw_goi), (cw_strike, cw_goi), pw_conc, cw_conc).
    pub fn wall_smoother_inputs(&self) -> ((f64, f64), (f64, f64), f64, f64) {
        let pw = self.pw_top();
        let cw = self.cw_top();
        let pw_total: f64 = self.put_walls.iter().map(|w| w.gamma_oi.abs()).sum();
        let cw_total: f64 = self.call_walls.iter().map(|w| w.gamma_oi.abs()).sum();
        let pw_top1 = self.put_walls.first().map(|w| w.gamma_oi.abs()).unwrap_or(0.0);
        let cw_top1 = self.call_walls.first().map(|w| w.gamma_oi.abs()).unwrap_or(0.0);
        let pw_conc = if pw_total > 0.0 { pw_top1 / pw_total } else { f64::NAN };
        let cw_conc = if cw_total > 0.0 { cw_top1 / cw_total } else { f64::NAN };
        (pw, cw, pw_conc, cw_conc)
    }

    /// γ×OI-weighted std dev of put wall strikes / ATR. Low = tight clustering = strong support.
    pub fn pw_dispersion_atr(&self, atr: f64) -> f64 {
        Self::weighted_wall_dispersion(&self.put_walls, atr)
    }

    pub fn weighted_wall_dispersion(walls: &[WallLevel], atr: f64) -> f64 {
        if walls.len() < 2 || atr <= 0.0 { return f64::NAN; }
        let total_goi = wall_gamma_oi_sum(walls);
        if total_goi <= 0.0 { return f64::NAN; }
        let wt_mean = walls.iter()
            .map(|w| (w.gamma_oi / total_goi) * w.strike)
            .sum::<f64>();
        let wt_var = walls.iter()
            .map(|w| (w.gamma_oi / total_goi) * (w.strike - wt_mean).powi(2))
            .sum::<f64>();
        wt_var.sqrt() / atr
    }

    /// γ×OI-weighted mean distance of wall strikes from `smoothed`, in ATR units.
    pub fn weighted_wall_dist_vs_smoothed(walls: &[WallLevel], smoothed: f64, atr: f64) -> Option<f64> {
        if walls.is_empty() || smoothed <= 0.0 || atr <= 0.0 { return None; }
        let total_goi = wall_gamma_oi_sum(walls);
        if total_goi <= 0.0 { return None; }
        let wt_mean = walls.iter()
            .map(|w| (w.gamma_oi / total_goi) * (w.strike - smoothed))
            .sum::<f64>();
        Some(wt_mean / atr)
    }

    /// Wall strikes sorted by gamma×OI descending (strongest first).
    fn strikes_ranked(walls: &[WallLevel]) -> Vec<f64> {
        let mut v: Vec<(f64, f64)> = walls.iter().map(|w| (w.strike, w.gamma_oi)).collect();
        v.sort_by(|a, b| cmp_f64(b.1, a.1));
        v.into_iter().map(|(s, _)| s).collect()
    }

    pub fn wide_call_strikes_ranked(&self) -> Vec<f64> { Self::strikes_ranked(&self.wide_call_walls) }

    /// Strongest wide call wall at least `min_otm_pct` above spot.
    pub fn strongest_wide_cw(&self, min_otm_pct: f64) -> f64 {
        let threshold = self.spot * (1.0 + min_otm_pct);
        self.wide_call_walls.iter()
            .filter(|w| w.strike >= threshold && w.gamma_oi > 0.0)
            .max_by(|a, b| cmp_f64(a.gamma_oi, b.gamma_oi))
            .map(|w| w.strike)
            .unwrap_or(0.0)
    }

    /// Narrow put wall + wide call wall at `min_otm_pct` OTM (WallBounce band).
    #[inline]
    pub fn wb_wall_pair(&self, min_otm_pct: f64) -> (f64, f64) {
        (self.pw(), self.strongest_wide_cw(min_otm_pct))
    }

    pub fn empty(spot: f64) -> Self {
        Self {
            spot,
            net_gex: 0.0,
            put_walls: vec![],
            call_walls: vec![],
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
            gamma_tilt: 0.0,
            net_vanna: 0.0,
            net_delta: 0.0,
        }
    }
}

// ─── Signals ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub enum Signal {
    #[serde(rename = "FLAT")]
    Flat,
    /// Swing signal: IV spike at put wall (puts ITM → dealer sell loop) followed
    /// by IV compression (puts back OTM → dealer forced to buy). Multi-day hold.
    #[serde(rename = "LONG_VANNA_FLIP")]
    LongVannaFlip,
    /// Calm-path: price dwells near put wall support with no active spike.
    #[serde(rename = "LONG_WALL_BOUNCE")]
    LongWallBounce,
}

impl Signal {
    pub fn is_flat(self) -> bool {
        matches!(self, Signal::Flat)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Flat => "FLAT",
            Signal::LongVannaFlip => "LONG_VANNA_FLIP",
            Signal::LongWallBounce => "LONG_WALL_BOUNCE",
        }
    }

    /// Short display label for charts, trade logs, and reason prefixes.
    pub fn short_name(self) -> &'static str {
        match self {
            Signal::Flat => "FLAT",
            Signal::LongVannaFlip => "VF",
            Signal::LongWallBounce => "WB",
        }
    }
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── VF gate enum + Rejection ────────────────────────────────────────────────

/// Every VannaFlip rejection reason — preconditions and gates.
/// Exhaustive: adding a variant forces all `match` sites to update at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VfGate {
    // Preconditions (checked before gates)
    NoPw,
    NoSpike,
    NoBaseline,
    // Gates (checked by VfGateCtx)
    AtrPct,
    SlowAtr,
    Tsi,
    TsiDead,
    CwWeak,
    PwWeak,
    SpreadWide,
    GexNorm,
    IvCompress,
    SpikeExpired,
    IvHigh,
    NoIv,
    SpikeVanna,
    SpikeGammaTilt,
    PwDrift,
    GammaTilt,
    RallyCap,
}

impl VfGate {
    /// All gate variants that can be checked via [`VfGateCtx`] (excludes preconditions).
    pub const SCAN_GATES: &[VfGate] = &[
        Self::AtrPct, Self::SlowAtr, Self::Tsi, Self::TsiDead,
        Self::CwWeak, Self::PwWeak, Self::SpreadWide,
        Self::GexNorm, Self::IvCompress,
        Self::SpikeVanna, Self::SpikeGammaTilt, Self::PwDrift, Self::GammaTilt,
        Self::RallyCap,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::NoPw => "vf_no_pw",
            Self::NoSpike => "vf_no_spike",
            Self::NoBaseline => "vf_no_baseline",
            Self::AtrPct => "vf_atr_pct",
            Self::SlowAtr => "vf_slow_atr",
            Self::Tsi => "vf_tsi",
            Self::TsiDead => "vf_tsi_dead",
            Self::CwWeak => "vf_cw_weak",
            Self::PwWeak => "vf_pw_weak",
            Self::SpreadWide => "vf_spread_wide",
            Self::GexNorm => "vf_gex_norm",
            Self::IvCompress => "vf_iv_compress",
            Self::SpikeExpired => "vf_spike_expired",
            Self::IvHigh => "vf_iv_high",
            Self::NoIv => "vf_no_iv",
            Self::SpikeVanna => "vf_spike_vanna",
            Self::SpikeGammaTilt => "vf_spike_gamma_tilt",
            Self::PwDrift => "vf_pw_drift",
            Self::GammaTilt => "vf_gamma_tilt",
            Self::RallyCap => "vf_rally_cap",
        }
    }
}

impl std::fmt::Display for VfGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone)]
pub struct Rejection {
    pub gate: VfGate,
    pub detail: String,
}

impl Rejection {
    pub fn new(gate: VfGate, detail: String) -> Self {
        Self { gate, detail }
    }
    pub fn plain(gate: VfGate) -> Self {
        Self { gate, detail: String::new() }
    }
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{}", self.gate)
        } else {
            write!(f, "{}({})", self.gate, self.detail)
        }
    }
}

// ─── Trade signal output ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalReason {
    /// Position should be held — never triggers a new entry.
    Hold,
    /// No entry / exit, with a diagnostic string (e.g. "no_spike", "vf_iv_rspike").
    Flat(String),
    /// New entry, with detail (e.g. "vanna_flip pw=$220 iv_peak=1.3 ...").
    Entry(String),
}

impl SignalReason {
    pub fn is_entry(&self) -> bool {
        matches!(self, Self::Entry(_))
    }

    /// Human-readable text for logging / diagnostics.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Hold => "hold",
            Self::Flat(s) | Self::Entry(s) => s,
        }
    }
}

impl std::fmt::Display for SignalReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct TradeSignal {
    pub timestamp: DateTime<Utc>,
    pub signal: Signal,
    pub price: f64,
    pub reason: SignalReason,
}

// ─── GEX stream phase ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "shared/generated/")]
#[serde(rename_all = "snake_case")]
pub enum GexPhase {
    Idle,
    Fetching,
    Live,
    Error,
}

impl std::fmt::Display for GexPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GexPhase::Idle => f.write_str("idle"),
            GexPhase::Fetching => f.write_str("fetching"),
            GexPhase::Live => f.write_str("live"),
            GexPhase::Error => f.write_str("error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(strike: f64) -> WallLevel {
        WallLevel { strike, gamma_oi: 1.0 }
    }

    fn sample_gex() -> GexProfile {
        GexProfile {
            spot: 150.0,
            net_gex: 100.0,
            put_walls: vec![wall(145.0), wall(140.0)],
            call_walls: vec![wall(155.0)],
            atm_put_iv: Some(0.25),
            wide_put_walls: vec![wall(130.0)],
            wide_call_walls: vec![wall(170.0), wall(180.0)],
            pw_com_dist_pct: 0.0,
            pw_near_far_ratio: 0.0,
            atm_gamma_dominance: 0.0,
            near_gamma_imbalance: 0.0,
            total_put_goi: 0.0,
            total_call_goi: 0.0,
            cw_depth_ratio: 0.0,
            gamma_tilt: 0.0,
            net_delta: 0.0,
            net_vanna: 0.0,
        }
    }

    #[test]
    fn gex_profile_wall_helpers() {
        let g = sample_gex();
        assert_eq!(g.pw(), 145.0);
        assert_eq!(g.cw(), 155.0);
        assert_eq!(g.pw_opt(), Some(145.0));
        assert_eq!(g.cw_opt(), Some(155.0));
        assert_eq!(wall_gamma_oi_sum(&g.put_walls), 2.0);
        assert_eq!(wall_gamma_oi_sum(&g.call_walls), 1.0);
        assert_eq!(g.wide_pw(), 130.0);
        assert_eq!(g.wide_cw(), 170.0);
        let (a, b) = g.vf_atr_pct_pair(1.0, 2.0);
        assert_eq!((a, b), (g.atr_pct(1.0), g.atr_pct(2.0)));
        assert_eq!(g.atm_put_iv_or_zero(), 0.25);
        assert_eq!(GexProfile::empty(100.0).atm_put_iv_or_zero(), 0.0);
        let (pw, cw) = g.wb_wall_pair(0.03);
        assert_eq!((pw, cw), (g.pw(), g.strongest_wide_cw(0.03)));
        let atr = 2.0;
        assert_eq!(g.pw_dist_atr_opt(atr), Some((150.0 - 145.0) / atr));
        assert_eq!(g.cw_dist_atr_opt(atr), Some((155.0 - 150.0) / atr));
        let (pd, cd, sp) = g.narrow_wall_atr_dists(atr);
        assert_eq!(pd.unwrap() + cd.unwrap(), sp.unwrap());
    }

    #[test]
    fn gex_profile_empty_walls_return_zero() {
        let g = GexProfile::empty(100.0);
        assert_eq!(g.pw(), 0.0);
        assert_eq!(g.cw(), 0.0);
        assert_eq!(g.pw_opt(), None);
        assert_eq!(g.cw_opt(), None);
        assert_eq!(g.wide_pw(), 0.0);
        assert_eq!(g.wide_cw(), 0.0);
    }

    #[test]
    fn pw_top_cw_top() {
        let g = sample_gex();
        assert_eq!(g.pw_top(), (145.0, 1.0));
        assert_eq!(g.cw_top(), (155.0, 1.0));
        let empty = GexProfile::empty(100.0);
        assert_eq!(empty.pw_top(), (0.0, 0.0));
    }

    #[test]
    fn wall_smoother_inputs_concentration() {
        let g = sample_gex();
        let ((pw_s, _), (cw_s, _), pw_conc, cw_conc) = g.wall_smoother_inputs();
        assert_eq!(pw_s, 145.0);
        assert_eq!(cw_s, 155.0);
        // 2 put walls with goi=1.0 each → top1/total = 0.5
        assert!((pw_conc - 0.5).abs() < 0.01);
        // 1 call wall → concentration = 1.0
        assert!((cw_conc - 1.0).abs() < 0.01);
    }

    #[test]
    fn pw_dispersion_atr_basic() {
        let g = sample_gex();
        let d = g.pw_dispersion_atr(1.0);
        assert!(d.is_finite() && d > 0.0, "dispersion should be positive: {d}");
    }

    #[test]
    fn pw_dispersion_atr_single_wall_is_nan() {
        let g = GexProfile {
            put_walls: vec![wall(100.0)],
            ..GexProfile::empty(100.0)
        };
        assert!(g.pw_dispersion_atr(1.0).is_nan());
    }

    #[test]
    fn option_row_valid_basic() {
        assert!(option_row_valid(150.0, 0.01, 0.25, 100.0, 155.0));
    }

    #[test]
    fn option_row_rejects_high_iv() {
        assert!(!option_row_valid(150.0, 0.01, 6.0, 100.0, 155.0));
    }

    #[test]
    fn option_row_rejects_zero_fields() {
        assert!(!option_row_valid(0.0, 0.01, 0.25, 100.0, 155.0));
        assert!(!option_row_valid(150.0, 0.0, 0.25, 100.0, 155.0));
        assert!(!option_row_valid(150.0, 0.01, 0.001, 100.0, 155.0)); // below MIN_IV
        assert!(!option_row_valid(150.0, 0.01, 0.25, 0.0, 155.0));
    }

    #[test]
    fn option_row_rejects_far_strike() {
        // strike 30% away from spot
        assert!(!option_row_valid(100.0, 0.01, 0.25, 100.0, 130.0));
    }

    #[test]
    fn fmt_pct_positive_and_negative() {
        assert_eq!(fmt_pct(5.25), "+5.25%");
        assert_eq!(fmt_pct(-3.10), "-3.10%");
        assert_eq!(fmt_pct(0.0), "+0.00%");
    }

    #[test]
    fn signal_is_flat() {
        assert!(Signal::Flat.is_flat());
        assert!(!Signal::LongVannaFlip.is_flat());
        assert!(!Signal::LongWallBounce.is_flat());
    }

    #[test]
    fn signal_as_str() {
        assert_eq!(Signal::Flat.as_str(), "FLAT");
        assert_eq!(Signal::LongVannaFlip.as_str(), "LONG_VANNA_FLIP");
        assert_eq!(Signal::LongWallBounce.as_str(), "LONG_WALL_BOUNCE");
    }

    #[test]
    fn signal_short_name() {
        assert_eq!(Signal::LongVannaFlip.short_name(), "VF");
        assert_eq!(Signal::LongWallBounce.short_name(), "WB");
    }

    #[test]
    fn signal_reason_is_entry() {
        assert!(SignalReason::Entry("test".into()).is_entry());
        assert!(!SignalReason::Hold.is_entry());
        assert!(!SignalReason::Flat("reason".into()).is_entry());
    }

    #[test]
    fn signal_reason_as_str() {
        assert_eq!(SignalReason::Hold.as_str(), "hold");
        assert_eq!(SignalReason::Entry("vf_entry".into()).as_str(), "vf_entry");
        assert_eq!(SignalReason::Flat("no_spike".into()).as_str(), "no_spike");
    }

    #[test]
    fn safe_ratio_and_opt_finite() {
        assert_eq!(safe_ratio(10.0, 2.0, 0.0), 5.0);
        assert_eq!(safe_ratio(10.0, 0.0, 7.0), 7.0);
        assert_eq!(opt_finite(1.0), Some(1.0));
        assert_eq!(opt_finite(f64::NAN), None);
    }
}
