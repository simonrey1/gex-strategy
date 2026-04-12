use std::collections::HashMap;

use crate::config::{StrategyConfig, Ticker};
use crate::data::thetadata_hist::ts_key;
use crate::strategy::entries::BarCtx;
use crate::strategy::indicators::{IndicatorValues, IncrementalIndicators};
use crate::strategy::signals::SignalState;
use crate::types::{GexProfile, OhlcBar, Signal, TradeSignal};

pub use crate::strategy::shared::*;
pub use crate::strategy::slot_sizing::SlotSizing;
pub use crate::strategy::warmup_result::WarmupResult;

// ─── Core engine ─────────────────────────────────────────────────────────────

/// Core strategy state shared by both live and backtest runners.
///
/// Encapsulates indicator computation, signal generation, and cooldown
/// tracking so that both runners use identical logic and can't diverge.
pub struct StrategyEngine {
    pub signal_state: SignalState,
    pub indicators: IncrementalIndicators,
    pub last_indicator_values: Option<IndicatorValues>,
    pub total_bars: u64,
}

impl StrategyEngine {
    pub fn new(config: &StrategyConfig) -> Self {
        let ss = SignalState::default();
        Self {
            signal_state: ss,
            indicators: IncrementalIndicators::new(config),
            last_indicator_values: None,
            total_bars: 0,
        }
    }

    /// Feed a new bar to indicators and increment the bar counter.
    /// Returns the current indicator values (owned) if warmed up.
    pub fn update_bar(&mut self, bar: &OhlcBar) -> Option<IndicatorValues> {
        self.total_bars += 1;
        let iv = self.indicators.update(bar);
        if let Some(ref v) = iv {
            self.last_indicator_values = Some(*v);
        }
        iv
    }

    /// Per-bar context for IV scan, spike diagnostics, and VF/WB evaluation (same wiring live/backtest).
    #[inline]
    pub fn bar_ctx<'a>(
        &'a self,
        bar: &'a OhlcBar,
        gex: &'a GexProfile,
        indicators: &'a IndicatorValues,
        config: &'a StrategyConfig,
        ticker: Ticker,
    ) -> BarCtx<'a> {
        BarCtx::new(&self.signal_state, bar, gex, indicators, config, ticker)
    }

    /// ATR regime ratio from the last warmed-up bar, or 1.0 if indicators are not ready yet.
    #[inline]
    pub fn atr_regime_ratio(&self) -> f64 {
        self.last_indicator_values
            .as_ref()
            .map(|v| v.atr_regime_ratio)
            .unwrap_or(1.0)
    }

    // warmup_trading_days lives on StrategyConfig now

    /// Warmup: replay historical bars + GEX through the full signal pipeline.
    /// No positions are opened — only internal state is populated.
    pub fn warm_up(
        &mut self,
        bars_with_gex: &[OhlcBar],
        gex_map: &HashMap<i64, GexProfile>,
        wall_smoother: &mut crate::strategy::wall_smoother::WallSmoother,
        hurst: &mut crate::strategy::hurst::HurstTracker,
        config: &StrategyConfig,
        ticker: crate::config::Ticker,
        verbose: bool,
    ) -> anyhow::Result<WarmupResult> {
        if bars_with_gex.is_empty() {
            anyhow::bail!("warmup: no bars provided");
        }

        // Verify bars have GEX coverage
        {
            let missing: Vec<_> = bars_with_gex.iter()
                .filter(|b| !gex_map.contains_key(&ts_key(&b.timestamp)))
                .collect();
            if !missing.is_empty() && verbose {
                eprintln!(
                    "[{}] warmup: {}/{} bars missing GEX (gex_map has {} keys). First missing: {}",
                    ticker, missing.len(), bars_with_gex.len(), gex_map.len(), missing[0].timestamp,
                );
            }
        }

        let mut last_processed_ms: i64 = 0;

        // Replay bars + GEX — full signal pipeline
        let p2_first = bars_with_gex.first().map(|b| &b.timestamp);
        let p2_last = bars_with_gex.last().map(|b| &b.timestamp);
        let mut signal_bars = 0usize;
        let mut iv_sum = 0.0f64;
        let mut gex_norm_sum = 0.0f64;
        for bar in bars_with_gex {
            let indicators = self.update_bar(bar);
            last_processed_ms = bar.timestamp.timestamp_millis();

            let mut signal_ran = false;
            if let Some(iv) = indicators {
                let key = ts_key(&bar.timestamp);
                if let Some(gex) = gex_map.get(&key) {
                    iv_sum += gex.atm_put_iv_or_zero();
                    gex_norm_sum += gex.net_gex;
                    self.process_gex_bar(ProcessGexBarCtx {
                        bar,
                        gex,
                        indicators: &iv,
                        smoother: wall_smoother,
                        hurst,
                        position: None,
                        config,
                        ticker,
                        verbose: false,
                    });
                    signal_bars += 1;
                    signal_ran = true;
                } else {
                    hurst.push(bar.close);
                }
            } else {
                hurst.push(bar.close);
            }
            if bar.is_eod() && !signal_ran {
                self.signal_state.prev_eod_close = bar.close;
            }
        }

        if verbose {
            let avg_iv = if signal_bars > 0 { iv_sum / signal_bars as f64 } else { 0.0 };
            let avg_gex = if signal_bars > 0 { gex_norm_sum / signal_bars as f64 } else { 0.0 };
            println!(
                "[warmup] {ticker} done: {} bars ({} signal) | {} .. {} | avg_atm_iv={:.4} avg_net_gex={:.1} iv_baseline_ema={:.6}",
                bars_with_gex.len(), signal_bars,
                p2_first.map_or("—".into(), |t| t.format("%Y-%m-%d %H:%M").to_string()),
                p2_last.map_or("—".into(), |t| t.format("%Y-%m-%d %H:%M").to_string()),
                avg_iv, avg_gex, self.signal_state.iv_baseline_ema,
            );
        }

        if self.last_indicator_values.is_none() {
            anyhow::bail!(
                "warmup {}: indicators not ready after {} bars (need {})",
                ticker, bars_with_gex.len(), config.min_indicator_bars(),
            );
        }

        let min_signal = config.min_signal_bars();
        if signal_bars < min_signal {
            anyhow::bail!(
                "warmup {}: {} GEX-matched signal bars, need at least {}. Missing ThetaData?",
                ticker, signal_bars, min_signal,
            );
        }
        if self.signal_state.iv_baseline_ema <= 0.0 && verbose {
            eprintln!("[{}] warmup: iv_baseline_ema never seeded — no valid IV in GEX data", ticker);
        }

        Ok(WarmupResult {
            bars_replayed: bars_with_gex.len(),
            signal_bars,
            last_processed_ms,
        })
    }

    /// Feed GEX wall data through the smoother into signal_state.
    fn apply_gex_walls(&mut self, gex: &GexProfile, ind: &IndicatorValues, smoother: &mut crate::strategy::wall_smoother::WallSmoother) {
        let w = smoother.update_from_gex(gex);
        self.signal_state.ingest_wall_smoother_gex_bar(gex, ind, smoother, &w);
    }

    /// Full GEX bar pipeline: smooth walls → wall trail (or hurst warmup) → signal.
    /// Guarantees identical ordering between backtest, live, and recovery.
    /// Returns (signal, wall_trail_outcome).
    pub fn process_gex_bar(&mut self, ctx: ProcessGexBarCtx<'_>) -> (TradeSignal, WallTrailOutcome) {
        self.apply_gex_walls(ctx.gex, ctx.indicators, ctx.smoother);
        let gex_norm = self.signal_state.gex_norm(ctx.gex.net_gex);
        let trail_inputs = ctx.trail_check_inputs(self.signal_state.smoothed_put_wall(), gex_norm);
        let trail = if let Some(p) = ctx.position {
            self.check_wall_trail(p, trail_inputs, ctx.config, ctx.hurst)
        } else {
            ctx.hurst.push(ctx.bar.close);
            WallTrailOutcome::Unchanged
        };
        let signal = self.signal_state.generate_signal(
            ctx.bar,
            ctx.gex,
            ctx.indicators,
            ctx.config,
            ctx.ticker,
            ctx.verbose,
        );
        (signal, trail)
    }

    /// Whether the core entry preconditions are met.
    /// Backtest should additionally check that no pending entry exists.
    pub fn can_enter(&self, g: CanEnterGate<'_>) -> bool {
        g.passes()
    }

    /// Common state update after a position is closed. Both runners must call
    /// this to keep holding and daily-risk state in sync.
    pub fn on_exit(&mut self, daily: &mut DailyState, pnl: f64, equity: f64) {
        self.signal_state.holding = Signal::Flat;
        daily.record_exit(pnl, equity);
    }

    /// Lock in the entry: set holding state, clear the spike so `check_entry`
    /// won't re-fire. Call this when the runner actually accepts the entry
    /// candidate (not at signal time — `check_entry` is pure).
    pub fn commit_entry(&mut self, signal: Signal, bar_close: f64) {
        self.signal_state.holding = signal;
        self.signal_state.entry_bar = self.signal_state.bar_index;
        self.signal_state.entry_price = bar_close;
        // Keep spike active — it expires naturally via iv_flip_max_bars.
        // Clearing it here would let intraday detection re-record the same episode.
    }

    /// Undo `commit_entry` when the fill is rejected (e.g. adaptive SL out of bounds).
    /// Without this, `holding` stays non-flat and the signal generator is stuck.
    pub fn undo_commit(&mut self) {
        self.signal_state.holding = Signal::Flat;
    }

    /// Common state update after a position is opened (fill price + daily counter).
    pub fn on_entry(&mut self, daily: &mut DailyState, entry_price: f64) {
        self.signal_state.entry_price = entry_price;
        daily.entries += 1;
    }

    /// Check wall-trailing SL for an open position on a strategy bar.
    /// Delegates to the standalone `check_trail` (single source of truth).
    pub fn check_wall_trail(
        &mut self,
        tf: TrailFields<'_>,
        inputs: crate::strategy::wall_trail::TrailCheckInputs,
        config: &StrategyConfig,
        hurst: &mut crate::strategy::hurst::HurstTracker,
    ) -> WallTrailOutcome {
        check_trail(tf, inputs, config, hurst)
    }

    /// Single entry-decision gate: checks `can_enter` and, if passed, builds
    /// the shared `EntryCandidateData`. Both runners MUST use this instead of
    /// calling `can_enter` + `build_entry_candidate` separately so that any
    /// new guard or field is automatically picked up everywhere.
    ///
    /// Runner-specific pre-guards (backtest: `pending_entry.is_none()`;
    /// live: `data_fresh`) are passed as `entry_when` to [`Self::process_gex_bar_with_entry_candidate`],
    /// or combined via [`Self::check_entry_candidate_when`].
    pub fn check_entry_candidate(&self, ctx: EntryCandidateCheckCtx<'_>) -> Option<EntryCandidateData> {
        if !self.can_enter(CanEnterGate::from(&ctx)) {
            return None;
        }
        Some(self.build_entry_candidate(&ctx.build))
    }

    /// Like [`Self::check_entry_candidate`], but returns `None` immediately when `when` is false.
    /// Lets live/backtest combine runner-specific gates (`data_fresh`, `pending_entry`, …) in one call.
    pub fn check_entry_candidate_when(
        &self,
        when: bool,
        ctx: EntryCandidateCheckCtx<'_>,
    ) -> Option<EntryCandidateData> {
        if !when {
            return None;
        }
        self.check_entry_candidate(ctx)
    }

    /// [`Self::process_gex_bar`] then [`Self::check_entry_candidate_when`] on the same bar/GEX inputs.
    /// Runners normally call [`Self::process_gex_bar_pipeline`] instead (builds [`TrailFields`] + `position_open`).
    ///
    /// If you call this directly, compute `position_open` before `trail_fields_for_position`
    /// (it mutably borrows the open position for [`TrailFields`]).
    pub fn process_gex_bar_with_entry_candidate(
        &mut self,
        pipe: &GexPipelineBar<'_>,
        config: &StrategyConfig,
        daily: &DailyState,
        wall_smoother: &mut crate::strategy::wall_smoother::WallSmoother,
        hurst: &mut crate::strategy::hurst::HurstTracker,
        position_open: bool,
        trail_fields: Option<TrailFields<'_>>,
    ) -> (TradeSignal, WallTrailOutcome, Option<EntryCandidateData>) {
        let bar_ctx = ProcessGexBarCtx::from_pipeline_bar(pipe, wall_smoother, hurst, trail_fields, config);
        let (signal, trail) = self.process_gex_bar(bar_ctx);
        let candidate = self.check_entry_candidate_when(
            pipe.entry_when,
            EntryCandidateCheckCtx {
                build: EntryCandidateBuildCtx {
                    signal: &signal,
                    bar: pipe.bar,
                    gex: pipe.gex,
                    indicators: pipe.indicators,
                    config,
                    ticker: pipe.ticker,
                },
                position_open,
                daily,
            },
        );
        (signal, trail, candidate)
    }

    /// Live/backtest: [`trail_fields_for_position`] then [`Self::process_gex_bar_with_entry_candidate`].
    /// `P` is the runner’s open-position type ([`HasTrailFields`]).
    pub fn process_gex_bar_pipeline<P: HasTrailFields>(
        &mut self,
        wall_smoother: &mut crate::strategy::wall_smoother::WallSmoother,
        hurst: &mut crate::strategy::hurst::HurstTracker,
        position: Option<&mut P>,
        pipe: GexPipelineBar<'_>,
        config: &StrategyConfig,
        daily: &DailyState,
    ) -> (TradeSignal, WallTrailOutcome, Option<EntryCandidateData>) {
        let position_open = position.is_some();
        let trail_fields = trail_fields_for_position(position);
        self.process_gex_bar_with_entry_candidate(
            &pipe,
            config,
            daily,
            wall_smoother,
            hurst,
            position_open,
            trail_fields,
        )
    }

    /// Build shared entry candidate data from the current engine state.
    /// Uses [`Self::bar_ctx`] so live/backtest stay aligned with IV scan and diagnostics.
    /// Prefer `check_entry_candidate` which bundles the can_enter gate.
    pub fn build_entry_candidate(&self, ctx: &EntryCandidateBuildCtx<'_>) -> EntryCandidateData {
        let bctx = ctx.bar_ctx(&self.signal_state);
        let tp_cap_atr = match ctx.signal.signal {
            Signal::LongWallBounce => {
                let cw = bctx.wb_trail_cw();
                if cw > bctx.bar.close && bctx.atr() > 0.0 {
                    (cw - bctx.bar.close) / bctx.atr()
                } else { 0.0 }
            }
            Signal::LongVannaFlip | Signal::Flat => 0.0,
        };

        EntryCandidateData {
            signal: ctx.signal.signal,
            reason: ctx.signal.reason.to_string(),
            entry_price: bctx.bar.close,
            atr_tsi: bctx.entry_atr_tsi(),
            adx: bctx.adx(),
            net_gex: bctx.net_gex(),
            gex_spot: bctx.gex_spot(),
            tp_cap_atr,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use crate::types::SignalReason;

    fn et(h: u32, m: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(2024, 6, 3).unwrap()
            .and_hms_opt(h, m, 0).unwrap()
            .and_utc()
    }

    // ── can_enter ───────────────────────────────────────────────────────

    fn daily(loss_hit: bool, entries: u32) -> DailyState {
        let mut d = DailyState::default();
        d.loss_limit_hit = loss_hit;
        d.entries = entries;
        d
    }

    fn gate<'a>(
        signal: Signal,
        reason: &'a SignalReason,
        position_open: bool,
        daily: &'a DailyState,
        config: &'a StrategyConfig,
        ts: chrono::DateTime<chrono::Utc>,
    ) -> CanEnterGate<'a> {
        CanEnterGate { signal, reason, position_open, daily, config, bar_timestamp: ts }
    }

    #[test]
    fn can_enter_basic() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let entry_reason = SignalReason::Entry("test".into());
        let morning = et(15, 0);
        let d = daily(false, 0);
        assert!(engine.can_enter(gate(Signal::LongVannaFlip, &entry_reason, false, &d, &cfg, morning)));
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &SignalReason::Hold, false, &d, &cfg, morning)));
    }

    #[test]
    fn can_enter_blocked_with_position() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let reason = SignalReason::Entry("test".into());
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &reason, true, &daily(false, 0), &cfg, et(15, 0))));
    }

    #[test]
    fn can_enter_blocked_by_daily_loss() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let reason = SignalReason::Entry("test".into());
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &reason, false, &daily(true, 0), &cfg, et(15, 0))));
    }

    #[test]
    fn can_enter_blocked_by_max_entries() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let reason = SignalReason::Entry("test".into());
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &reason, false, &daily(false, cfg.max_entries_per_day), &cfg, et(15, 0))));
    }

    #[test]
    fn can_enter_blocked_before_open() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let reason = SignalReason::Entry("test".into());
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &reason, false, &daily(false, 0), &cfg, et(13, 0))));
    }

    #[test]
    fn can_enter_blocked_after_close() {
        let cfg = StrategyConfig::default();
        let engine = StrategyEngine::new(&cfg);
        let reason = SignalReason::Entry("test".into());
        assert!(!engine.can_enter(gate(Signal::LongVannaFlip, &reason, false, &daily(false, 0), &cfg, et(19, 30))));
    }

    // ── check_wall_trail (integration via StrategyEngine) ───────────────

    #[test]
    fn check_wall_trail_returns_ratcheted() {
        let mut cfg = StrategyConfig::default();
        cfg.exit_width_atr = 2.0;
        cfg.hurst_exhaust_threshold = 0.0;
        let mut engine = StrategyEngine::new(&cfg);
        let mut sl = 90.0;
        let mut hw = 0.0;
        let mut hc = 100.0;
        let mut heb = 0u32;
        let mut hurst = crate::strategy::hurst::HurstTracker::new(100);
        let tf = TrailFields {
            stop_loss: &mut sl, highest_put_wall: &mut hw,
            highest_close: &mut hc, hurst_exhaust_bars: &mut heb, entry_price: 100.0, tp: 130.0,
            signal: crate::types::Signal::LongVannaFlip,
        };
        let outcome = engine.check_wall_trail(
            tf,
            crate::strategy::wall_trail::TrailCheckInputs::new(
                107.0,
                crate::types::BarVolRegime::new(110.0, 1.0, 1.0),
                0.0,
            ),
            &cfg,
            &mut hurst,
        );
        assert!(matches!(outcome, WallTrailOutcome::Ratcheted { .. }));
        assert!(sl > 90.0);
    }

    #[test]
    fn check_wall_trail_unchanged_when_no_gain() {
        let cfg = StrategyConfig::default();
        let mut engine = StrategyEngine::new(&cfg);
        let mut sl = 90.0;
        let mut hw = 0.0;
        let mut hc = 100.0;
        let mut heb = 0u32;
        let mut hurst = crate::strategy::hurst::HurstTracker::new(100);
        let tf = TrailFields {
            stop_loss: &mut sl, highest_put_wall: &mut hw,
            highest_close: &mut hc, hurst_exhaust_bars: &mut heb, entry_price: 100.0, tp: 130.0,
            signal: crate::types::Signal::LongVannaFlip,
        };
        let outcome = engine.check_wall_trail(
            tf,
            crate::strategy::wall_trail::TrailCheckInputs::new(
                95.0,
                crate::types::BarVolRegime::new(101.0, 1.0, 1.0),
                0.0,
            ),
            &cfg,
            &mut hurst,
        );
        assert!(matches!(outcome, WallTrailOutcome::Unchanged));
    }

    // ── commit / undo / on_entry / on_exit ──────────────────────────────

    #[test]
    fn commit_and_undo_entry() {
        let cfg = StrategyConfig::default();
        let mut engine = StrategyEngine::new(&cfg);
        engine.commit_entry(Signal::LongVannaFlip, 150.0);
        assert_eq!(engine.signal_state.holding, Signal::LongVannaFlip);
        engine.undo_commit();
        assert_eq!(engine.signal_state.holding, Signal::Flat);
    }

    #[test]
    fn on_entry_increments_daily() {
        let cfg = StrategyConfig::default();
        let mut engine = StrategyEngine::new(&cfg);
        let mut daily = DailyState::new(0.02);
        engine.on_entry(&mut daily, 150.0);
        assert_eq!(daily.entries, 1);
        assert!((engine.signal_state.entry_price - 150.0).abs() < 0.01);
    }

    #[test]
    fn on_exit_records_pnl() {
        let cfg = StrategyConfig::default();
        let mut engine = StrategyEngine::new(&cfg);
        let mut daily = DailyState::new(0.02);
        engine.on_exit(&mut daily, -50.0, 10_000.0);
        assert!((daily.realized_pnl - -50.0).abs() < 0.01);
    }

    // ── rank_and_dedup ──────────────────────────────────────────────────

    #[test]
    fn rank_and_dedup_sorts_descending_and_dedupes() {
        use crate::config::Ticker;
        struct C { t: Ticker, score: f64 }
        impl RankedCandidate for C {
            fn ticker(&self) -> Ticker { self.t }
            fn rank_score(&self) -> f64 { self.score }
        }
        let mut v = vec![
            C { t: Ticker::AAPL, score: 1.0 },
            C { t: Ticker::GOOG, score: 3.0 },
            C { t: Ticker::AAPL, score: 2.0 }, // dupe, lower score
            C { t: Ticker::MSFT, score: 2.5 },
        ];
        rank_and_dedup(&mut v);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].t, Ticker::GOOG);
        assert_eq!(v[1].t, Ticker::MSFT);
        assert_eq!(v[2].t, Ticker::AAPL);
        assert!((v[2].score - 2.0).abs() < 0.01); // kept the best AAPL
    }
}
