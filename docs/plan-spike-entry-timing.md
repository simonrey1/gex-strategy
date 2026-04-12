# Plan: Spike Detection & Entry Timing Improvements

## Context

Spike windows time bottoms well — IV surges while price is at/below the smoothed put wall, and the subsequent IV compression creates a mechanical dealer buy-back tailwind (VannaFlip thesis). The 50-bar (12.5h) window captures this, but entries are often blocked by VF gates or arrive late.

---

## Part A — Spike Detection Itself

### Current Detection Logic

`SpikeCheckInputs::check()` in `iv_eligibility.rs` — fires when **all three** hold on a single 15-min bar:

1. **IV threshold**: `iv_daily_high > iv_baseline_ema × eff_iv_spike_mult(regime)`
   - `iv_daily_high`: highest `atm_put_iv` seen today (resets each morning)
   - `iv_baseline_ema`: ~5-day EOD EMA of ATM put IV (frozen during spike days to avoid contamination)
   - `eff_iv_spike_mult`: default 3.5×, scaled by `regime^0.65` (slightly higher bar in high-vol regimes)
2. **Near put wall**: `(spot − smoothed_pw) / ATR ≤ regime_slack`
   - `regime_slack = max(0, 1/regime − 1)` — in low-vol (regime 0.5): slack ≈ 1 ATR; in high-vol (regime 2.0): slack ≈ 0
3. **Min volatility**: `ATR / spot × 100 ≥ spike_min_atr_pct` (0.30%)

**Episode semantics**: once conditions fire, `spike_episode_active = true` and the spike bar/level are locked. If conditions stop being met on any bar, the episode is cleared. A new trigger starts a fresh episode. Within an episode, re-checking conditions doesn't overwrite the existing spike — it's one-shot.

### Limitations & Blind Spots

| Issue | Description |
|-------|-------------|
| **Single IV metric** | Only uses `atm_put_iv` (average put IV within ±5% of spot). Skew, term structure, and individual strike IVs are ignored |
| **Daily-high only** | `iv_daily_high` captures the peak but not the shape — a brief flash spike and a sustained grind look the same |
| **No vanna/delta signal** | `net_vanna` and `net_delta` exist on `GexProfile` but are unused in detection. The vanna flip thesis is literally about vanna, yet the spike trigger is pure IV |
| **Binary put-wall proximity** | `near_put_wall()` is yes/no. A spike 0.1 ATR below PW and 0.9 ATR below PW are treated the same |
| **Baseline can stale** | `iv_baseline_ema` freezes when spiking (to avoid contamination), but after a multi-week elevated-IV regime, the "baseline" may be stale/low, making the multiplier easy to trip |
| **No GEX regime check** | Spike fires regardless of GEX magnitude. A spike with net_gex = −$10B (massive dealer short gamma) is fundamentally different from net_gex ≈ 0 |
| **Volume blind** | No check on options volume or OI change. A real capitulation spike should come with outsized put volume |
| **Gamma tilt unused** | `gamma_tilt` (call_goi − put_goi normalized) is computed but not part of detection. Strong negative gamma tilt at spike time = more dealer amplification |

### Proposed Improvements to Detection

#### A1. Vanna Confirmation

**Problem:** The spike is pure IV. But the VF thesis is about *vanna flow* — dealers who sold puts are short vanna, and when IV drops they buy back stock. If net_vanna is near zero at spike time, there's no vanna fuel for the subsequent flip.

**Data available:** `GexProfile.net_vanna` (computed every 15-min GEX bar).

**Approach:** Require `net_vanna < -threshold` at spike time (dealers meaningfully short vanna). Sweepable: `spike_min_neg_vanna` (absolute or normalized by EMA).

**Risk:** May filter valid spikes in tickers with structurally low options activity. Needs per-ticker or normalized threshold.

**Effort:** Low — data exists, one additional condition in `check()`.

#### A2. IV Velocity / Acceleration (Not Just Level)

**Problem:** `iv_daily_high` is a static level. A spike from 0.15 → 0.50 in 2 bars is more meaningful than one that drifted from 0.15 → 0.50 over 20 bars. The speed of IV rise indicates *panic urgency*.

**Data available:** `atm_put_iv` per bar (tracked as `iv_daily_high`, but bar-level IV is accessible from `gex.atm_put_iv`).

**Approach:** Track `iv_prev_bar` in `SignalState`. Compute bar-over-bar IV change. Require the IV to have risen by ≥ X% in the last N bars (e.g. +50% from 3 bars ago). Alternative: IV acceleration (second derivative positive — still accelerating vs decelerating).

**Effort:** Low — one new field, simple computation.

#### A3. Gamma Tilt Gate

**Problem:** `gamma_tilt = (call_goi − put_goi) / (call_goi + put_goi)`. Negative = put gamma dominates = dealers amplify downside. Currently unused at detection time.

**Approach:** Require `gamma_tilt < -0.1` (or sweepable threshold) at spike time. This confirms the dealer positioning that creates the amplification-then-reversal dynamic.

**Effort:** Very low — one line in `check()`.

#### A4. Put Wall Distance as Continuous Signal

**Problem:** `near_put_wall()` is boolean. Price at 0.01 ATR above PW and 0.95 ATR above PW both return true (when slack allows), but the quality is very different.

**Approach:** Replace boolean with a continuous score: `pw_proximity = max(0, 1 − dist/max_dist)`. Use this score to modulate the IV spike threshold: tighter to PW → lower IV mult required (closer to capitulation). Further → need a larger spike.

```
eff_mult = base_mult − pw_proximity × softening_factor
```

Sweepable: `spike_pw_proximity_softening`.

**Effort:** Low — replaces the boolean, same data.

#### A5. Baseline EMA Staleness Guard

**Problem:** `iv_baseline_ema` is frozen during spike days. If a ticker enters a multi-week elevated-IV regime (e.g. earnings season, macro stress), the baseline stays artificially low, making the 3.5× threshold trivially easy. This creates "false spikes" — IV is high but it's structural, not panic.

**Approach:** Track `bars_since_baseline_update`. If the baseline hasn't been updated for > N days (e.g. 10), either:
- Force a slow partial update: `baseline = max(baseline, 0.5 × current_iv)` to prevent drift
- Raise the effective multiplier: `eff_mult × (1 + stale_days/20)`

**Effort:** Low — one new counter field.

#### A6. GEX Magnitude at Spike Time

**Problem:** A spike with `net_gex = −$10B` (massive short gamma) has much more reversal fuel than one with `net_gex ≈ 0`. Currently GEX only matters *after* the spike (via `GexNorm` entry gate).

**Approach:** Add a condition: `gex_norm < -X` at spike detection time (dealers meaningfully short gamma). This ensures the spike represents a dealer positioning extreme, not just a random IV pop.

**Effort:** Very low — `gex_norm()` already computed.

#### A7. Multi-Timeframe IV (Skew / Term Structure)

**Problem:** `atm_put_iv` is a single number. Real panic shows up as:
- **Skew steepening**: OTM put IV rising faster than ATM
- **Term structure inversion**: near-term IV >> far-term (panic is front-loaded)

**Data available:** The raw options data (`all_greeks`) has per-strike, per-expiry IVs. The GEX builder could compute these features.

**Approach:** In `gex_builder.rs`, compute:
- `put_skew_25d`: IV(25Δ put) − IV(ATM) as a ratio
- `term_structure_slope`: avg IV(< 30 DTE) / avg IV(> 60 DTE)

Add these to `GexProfile`. Then: spike = IV spike + `put_skew_25d > threshold` or `term_slope > threshold`.

**Effort:** High — new GEX computation, new fields, cache bump. But very high signal quality.

#### A8. Episode Duration Flexibility

**Problem:** Episode clears the instant conditions stop being met (any bar where the 3 conditions don't hold). In volatile environments, IV can dip briefly below threshold then surge again — the current logic treats this as two separate spikes, losing context.

**Approach:** Add a `spike_grace_bars` cooldown (e.g. 3 bars). When conditions stop being met, wait N bars before clearing. If conditions re-trigger within the grace period, extend the existing episode.

**Effort:** Low — one counter in `SpikeConditions::apply`.

---

## Part B — Entry Timing Within Spike Windows

## Current Spike Window Mechanics

**Opens** when all three hold simultaneously on a 15-min bar:
1. `iv_daily_high > iv_baseline_ema × eff_iv_spike_mult(regime)` (default 3.5×)
2. Spot at/below smoothed put wall (regime-adaptive slack)
3. `ATR% ≥ spike_min_atr_pct` (0.30%)

**Closes** after 50 bars or when episode conditions stop being met.

**Entry gates** within the window (most common blockers from missed-entries analysis):
- `IvCompress`: binary `ratio ≤ 0.5` — IV hasn't unwound enough
- `PwWeak` / `CwWeak`: wall structure degraded
- `GexNorm`: GEX still deeply negative
- `TsiMax`: TSI already too high (bounce happened without us)

## Proposed Improvements (ranked by expected impact)

### B1. IV Compression Velocity Gate

**Problem:** `compress_ratio ≤ 0.5` is binary. A fast IV collapse (dealers unwinding fast) should qualify earlier than a slow grind.

**Data available:** `spike_cum_iv_drop`, `spike_prev_iv` already tracked in `SignalState`.

**Approach:** Compute IV drop velocity = `spike_cum_iv_drop / bars_since_spike`. When velocity exceeds a threshold (e.g. 0.005/bar), relax the compression ratio requirement (e.g. allow entry at 0.6 instead of 0.5). Sweepable param: `vf_compress_velocity_threshold`.

**Effort:** Low — data exists, just a new gate condition.

### B2. Put Wall Reclaim Confirmation

**Problem:** Spike fires when price is at/below PW. Entering immediately risks catching a falling knife. Waiting for price to reclaim PW with conviction filters false bottoms.

**Data available:** `smoothed_put_wall()`, bar close/low/high.

**Approach:** After spike, track consecutive bars where `close > smoothed_pw`. Require N bars (e.g. 2) above PW before allowing entry. New `SignalState` field: `bars_above_pw_since_spike`. Sweepable: `vf_min_pw_reclaim_bars`.

**Effort:** Low — simple counter, reset on each spike.

### B3. GEX Flip Detection (Derivative Signal)

**Problem:** `GexNorm` gate is a ceiling (blocks when GEX too negative). But the *transition* from negative to less-negative is the actual vanna flip — dealers switching from selling to buying.

**Data available:** `net_gex`, `gex_abs_ema`.

**Approach:** Track `gex_norm` delta (bar-over-bar change). When `gex_norm` was < -1.5 and crosses above -0.5 (configurable), treat as a "flip confirmed" signal. Could be a gate softener or a signal quality booster. New fields: `prev_gex_norm`, `gex_flip_bar`.

**Effort:** Medium — new state tracking, new gate logic.

### B4. Gamma Position Floor

**Problem:** `gamma_pos` (0=at PW, 1=at CW) is computed but has no lower bound for entry. Entering when gamma_pos ≈ 0 means price is still pinned at the bottom.

**Data available:** `compute_gamma_pos()` already in `VfGateCtx`.

**Approach:** Add `vf_min_gamma_pos` param (default 0.15–0.25). Entry only when price has lifted off PW into the channel. Trivial sweep parameter.

**Effort:** Very low — one-line gate addition.

### B5. Spike Consumption Metrics as Gates

**Problem:** `spike_mfe_atr`, `spike_mae_atr`, `spike_cum_return_atr` are tracked but unused for entry decisions.

**Approach:**
- **MAE peaked:** Enter after MAE has plateaued (no new lows for N bars). Signals selling pressure absorbed.
- **MFE growing:** `spike_mfe_atr > spike_mae_atr` — the bounce exceeds the drawdown.
- **Cum return positive:** `spike_cum_return_atr > 0` — net up from spike close.

These could combine into a "spike quality score" that gates or ranks entries.

**Effort:** Medium — need to track "bars since last MAE update" as new state.

### B6. Adaptive Window Length

**Problem:** Fixed 50-bar window. Fast IV compression consumes the setup in 10 bars; slow grinds might need 60+.

**Approach:** Window closes when `compress_ratio < 0.3` (IV fully unwound, opportunity consumed) OR at `max_window_bars` (still needed as safety cap). Could also extend window if compression is slow but still trending.

**Effort:** Medium — changes `close_spike_window_if_expired` logic and chart rendering.

### B7. Hurst Mean-Reversion Signal

**Problem:** Hurst exponent < 0.4 during spike window indicates strong mean-reversion tendency, which supports the VF thesis. Currently unused for entry timing.

**Data available:** `hurst` field on `TickerState`.

**Approach:** Use Hurst < 0.4 as a confidence booster — could relax other gates slightly (e.g. allow higher `compress_ratio`) when mean-reversion is structurally likely.

**Effort:** Low — data exists, just conditional relaxation.

## Evaluation Methodology

For each change:
1. Run `-v 1` with `--missed-entries` to check how many previously-missed best entries now get captured
2. Sweep the new param to find plateau (not peak)
3. Check portfolio Sharpe/Sortino/MDD in full-period + sub-period windows
4. Verify no regression on existing entries (non-reg test)

## Recommended Priority

**Detection (Part A):** Start with **A6 (GEX magnitude)** + **A3 (gamma tilt)** — very low effort, directly validate the dealer positioning that the VF thesis depends on. Then **A1 (vanna confirmation)** for the actual flow signal. **A5 (baseline staleness)** is a robustness fix that should be done regardless.

**Entry timing (Part B):** Start with **B4 (gamma pos floor)** and **B1 (IV compression velocity)** — lowest effort, highest signal.
