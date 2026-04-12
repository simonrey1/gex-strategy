# Notes for AI Agents

## General

- **Rust backend**: all code in `src/`. No TypeScript backend.
- **15-min bar strategy**: `BAR_INTERVAL_MINUTES = 15`. All bar indices (`BarIndex`, `IV_LOOKBACK_BARS`, `bars_since_spike`, etc.) count 15-min bars, not 1-min. Duration in seconds = bars × 15 × 60.
- **USD only, long-only**: US equities only. Never generate short signals. Position sizing uses IBKR `AvailableFunds`.
- **Optimization target**: maximize net return. MDD ≤ 10% is acceptable; don't over-optimize for Sharpe at the cost of returns.
- **Two entry signals**: VF (VannaFlip): IV spike → compression → dealer vanna buy-back. WB (WallBounce): calm-path zone-dwell near put wall. WB is currently enabled only for JPM.

### VannaFlip pipeline

When IV spikes — options fear surges — dealers who sold puts hedge aggressively.
When the panic fades and IV compresses, those same dealers unwind: they buy back
stock, creating a mechanical tailwind. The strategy detects the storm, then waits
for the calm before entering.

```
Every 15-min bar
│
├─ 1. SPIKE DETECTION  (iv_eligibility.rs → SpikeCheckInputs::check)
│     All three must hold on the same bar:
│     ├─ iv_daily_high > iv_baseline_ema × eff_iv_spike_mult(regime)
│     │   iv_daily_high : highest atm_put_iv today (resets each morning)
│     │   iv_baseline_ema : ~5-day EOD EMA (frozen during spike days)
│     │   eff_mult : default 3.5×, scaled by regime^0.65
│     ├─ near_put_wall : (spot − smoothed_pw) / ATR ≤ regime_slack
│     └─ atr_ok : ATR/spot × 100 ≥ spike_min_atr_pct (0.30%)
│     → sets spike_episode_active, locks spike_bar + spike_level
│     Episode clears if conditions stop being met on any bar.
│
│  ┌── 50-bar window (12.5h) from spike to find an entry ──┐
│
└─ 2. VF ENTRY GATES  (wall_bounce.rs → VfCtx::evaluate)
      Checked every bar within the window. All must pass simultaneously:
      ├─ IvCompress : iv_now / spike_level ≤ 0.5 (IV halved — fear fading)
      ├─ AtrPct : vf_min_atr_pct ≤ ATR% < vf_max_atr_pct (0.50 — choppiness settling)
      ├─ SlowAtr : slow ATR% < vf_max_slow_atr_pct (0.55)
      ├─ DeadZone : TSI not in dead zone (low ADX + low TSI)
      ├─ SpreadWide : wall spread ≤ max (walls not too far apart)
      ├─ CwWeak : narrow CW not persistently below smoothed CW
      ├─ PwWeak : narrow PW not persistently below smoothed PW
      ├─ GexNorm : net_gex / gex_abs_ema ≤ threshold
      ├─ Tsi : TSI ≤ eff_vf_max_tsi(regime)
      └─ SpikeExpired : bars_since_spike ≤ 50 & atm_put_iv > 0
      → first bar where all pass: entry candidate created, position opened
```

**`iv_daily_high`**: uses today's *peak* IV, not the current reading. A 9:35am
flash spike keeps the window open all day — the panic already happened.

**IvCompress + AtrPct** carry most of the Sharpe. They are the "calm after the
storm" filters: IV compression waits for fear to fade, ATR cap waits for actual
price choppiness to settle.

### IV scan oracle

`--missed-entries` runs a parallel oracle (`IvScanTracker`) that measures spike
quality independently of the VF entry gates.

**What it does** (`iv_scan.rs`):
1. Runs its own Stage 1 spike detection — same `SpikeCheckInputs::check()`.
2. Calls `IvCompressionInputs::is_eligible()` — currently a lightweight gate:
   has_spike + within 50 bars + IV data present. If made smarter, this gate
   would also affect b/w (it filters which bars become scan entries).
3. Opens a scan window of 50 bars. Enters at **every eligible bar** — skips
   all VF entry gates (no IvCompress, AtrPct, TSI, GEX norm, etc.).
4. Each simulated entry **uses our exact exit mechanics** (SL, TP, wall trailing,
   Hurst exhaust, early TP). The exit P&L is real, not raw price runup.
5. Classifies results as **best** (≥3% profit, efficient, low DD) or **worst**.

**b/w(total) ratio** = total_best / total_worst. Measures the raw quality of
detected spike windows. Affected by:
- Stage 1 changes (spike detection) — alter which spike windows exist
- `is_eligible()` changes — alter which bars within windows become scan entries
- NOT affected by VF entry gate changes (oracle skips them entirely)

**Why scan "Best" entries don't always translate to portfolio gains**:
the oracle simulates each entry in isolation (unlimited capital, no position
limit). In the real backtest, `max_open_positions = 3` causes **slot
displacement**: rescuing a blocked entry often takes the slot of a better
entry that would have fired a few bars later. Repeatedly tested (2026-04):
CW-rescue, IV-compress rescue, rally-cap filters all showed net-negative
impact despite targeting oracle-confirmed Best entries.

### What does NOT work for improving Sharpe

**Do NOT analyze scan CSV / missed-entries statistics to design new filters.**
Repeatedly tried (2026-04): computing feature medians for Best vs Worst in
`scan_data.csv`, identifying discriminating thresholds (`cum_return_atr`,
`tsi + bars_since_spike`, `gamma_tilt`, `spike_vanna`, `pw_drift_atr`), then
implementing those as gates. Every single one degraded portfolio Sharpe when
swept because:
1. Scan statistics ignore slot displacement — what looks like a good filter
   on isolated entries hurts when position limits interact.
2. Existing gates already implicitly capture the same signals — the remaining
   entries that pass all gates are already well-selected.
3. New threshold filters remove more marginal winners than bad trades.

**Instead**: modify Rust code directly (new combination logic, adaptive
thresholds, structural changes to gate interactions) and validate immediately
with `--sweep` + `-v 0`. If the sweep shows no improvement at any param value,
move on — do not iterate on the same idea with different analysis angles.

- **Incremental indicators**: `IncrementalIndicators` in `indicators.rs` is O(1)/bar. Do NOT revert to batch.
- **Shared types**: `OhlcBar`, `GexProfile`, `Signal`, etc. in `src/types.rs`. Dashboard types in `src/live/dashboard_types.rs`, auto-generated via ts-rs.
- **Keep runner.rs lean**: runners are orchestrators only. New structs/helpers go in dedicated files.
- **One struct = one file**: every non-trivial struct with its own `impl` belongs in its own file.
- **DRY**: never duplicate computation or struct definitions. One source of truth for Rust↔TS.
- **Hard data requirements**: when walls exist, all dependent data must be present. Missing data = `panic!`, never `unwrap_or(0.0)`.

## Data layout

Two separate directories, **never conflate them**:

```
data/unified/{TICKER}/                   # Parquet files (rebuilt from downloads)
  bars_1m.parquet                        # Unified 1-min OHLCV (zstd, footer has processed_dates)
  gex_15m_v{V}.parquet                   # Unified GEX profiles (zstd, footer has processed_months)

data/downloads/backtest/{TICKER}/{YYYY-MM}/{YYYY-MM-DD}/  # Per-day source files (~29 GB total)
  processed_bars1m.json                  # IBKR 1-min OHLCV (epoch timestamps)
  raw_options_wide_v{N}.json.gz          # 15-min all_greeks (raw ThetaData)
```

At startup, `ensure_binary_cache` migrates per-day files into unified parquet if they don't exist yet.

Bumping `V` forces GEX recompute. Bumping `N` forces re-download.

### Backtest data

Parquet files in `data/unified/` are tracked via Git LFS (~315 MB). Run `git lfs pull` after cloning to download them.

## Backtest CLI

`cargo run --release --bin backtest -- --help` for all flags.

**CLI naming**: flags match config field names (hyphens for underscores).

**Negative CLI values**: use `=` syntax: `--param1=-0.5`.

**Sweeping**: prefer `--sweep` (single process, shares in-memory data cache, much faster than a shell loop). Values are comma-separated, negatives work. Supports multi-param grid sweeps (space-separated specs → cartesian product):

```bash
# 1D sweep
cargo run --release --bin backtest -- --sweep "vf_min_atr_pct=0.20,0.25,0.30,0.35,0.40"
cargo run --release --bin backtest -- --sweep "exit_width_atr=3.5,4.0,4.5,5.0" --ticker AAPL

# 2D grid sweep (4×3 = 12 runs)
cargo run --release --bin backtest -- --sweep "exit_width_atr=4.0,4.25,4.5,4.75 sl_tsi_adapt=1.5,1.75,2.0"
```

Param names use underscores (matching config field names). Any `StrategyOverrides` field works.

**Fallback shell loop** (when `--sweep` isn't suitable, e.g. sweeping non-strategy flags):

```bash
for val in 1 2 3 4 5; do
  echo "=== param=$val ==="
  cargo run --release --bin backtest -- -v 0 --some-flag $val
done
```

`-v 0` gives a single summary line. `-v 1` is NOT for sweeps.

**Missed entries**: `--missed-entries` shows the best IV-scan entries that VF gates blocked, with per-gate stats. Add `-p 8080` for a dashboard with mini charts per missed entry.

**Overfit report**: `--overfit-report` generates `potential_overfits.md` with half-period sweeps, 4-period stability, and per-ticker analysis. Takes ~10 min.

### Sweep discipline

Goal: **stable plateaus**, not peak Sharpe.

**Structural param linking** prevents H1/H2 divergence:
- **Regime-adaptive thresholds**: GEX norm, Hurst exhaust, PW-SPW thresholds scale with `atr_regime_ratio` via `eff_*` methods.
- **Plateau clamping**: `validate_and_clamp()` enforces safe ranges.

When adding new params: prefer linking to existing regime-adaptive methods over creating new independent knobs.

## Evaluating tickers for the portfolio

A ticker doesn't need high standalone Sharpe to be worth adding. What matters is **marginal portfolio impact**: does adding it improve portfolio Sharpe/Sortino/net without blowing MDD past 10%?

Low-frequency tickers (5-20 trades/8yr) with high avg trade quality (+3-5% avg) are valuable: they fill idle capital slots without degrading portfolio quality.

## Non-regression tests

**Portfolio** (`tests/portfolio_nonreg.rs`): insta inline snapshots, exact-match.
- **Check**: `cargo test --release --test portfolio_nonreg`
- **Update**: `INSTA_UPDATE=always INSTA_FORCE_PASS=1 cargo test --release --test portfolio_nonreg && cargo insta accept`

## Live / backtest parity

Both runners must stay behaviourally consistent.
1. **Shared helpers**: position math in `engine.rs`, GEX in `gex_builder.rs`
2. **Config-driven**: every threshold from `StrategyConfig`, never hardcode in one path
3. **Risk rules must match**: `max_open_positions`, `daily_loss_limit_pct`, `max_entries_per_day`
4. **Entry decision gate**: `StrategyEngine::check_entry_candidate()` bundles `can_enter` + `build_entry_candidate`. Both runners MUST call this single method.
5. **Exhaustive match on enums**: use explicit match arms (no `_ => {}`).
6. **Intentional differences** (don't "fix"): execution delay, SL/TP checking (intra-bar vs IBKR bracket), stale-data emergency close, slot counting (backtest counts pending entries, live does not)

## TypeScript bindings

Auto-generated via ts-rs. **Never hand-edit** `bindings/shared/generated/*.ts`.

**How it works**: every `#[ts(export)]` struct generates an `export_bindings_<name>` test. Running tests writes `.ts` files to `bindings/shared/generated/`. The `shared/types.ts` re-exports them with `Numberify<>` (bigint → number).

**Regenerate bindings** (fast, skips all other tests):

```bash
cargo test --release -- export_bindings
```

Output: `bindings/shared/generated/*.ts` (one file per exported struct).

**Full workflow** after editing a Rust struct with `#[derive(TS)]`:

1. `cargo test --release -- export_bindings` — regenerate `.ts` files
2. If new struct: add import + `Numberify<>` re-export in `shared/types.ts`
3. `cd dashboard && npx vitest run && npm run build` — verify + rebuild

## Live deployment

Server: `ssh root@204.168.255.35` — repo at `/root/gex` (main branch).

**Deploy workflow** (from server):

```bash
cd /root/gex
git pull origin main
kube/play.sh down        # tear down running pod
kube/play.sh prod        # build image + start pod (IB Gateway + ThetaData + strategy)
kube/play.sh logs        # follow strategy logs
kube/play.sh logs-theta  # follow ThetaData logs
kube/play.sh ps          # list pods/containers
```

`play.sh prod` builds the container image, starts all three containers in a pod,
and waits for IB Gateway + ThetaData to be reachable. Dashboard at `https://siimo.org`.

## GEX walls

All built from 15-min `all_greeks` data (all expirations, ±25% strikes):
- **Narrow** = highest γ×OI strikes near spot. Moves fast.

## Backtest state / results files

Per-ticker state files (JSON) are written when `--save-json` is passed:

```
data/results/state-{TICKER}-{YYYY-MM-DD-HH-MM-SS}.json
data/results/latest-{TICKER}.json    # symlink to most recent
```

Portfolio-level state: `data/results/portfolio-{timestamp}.json`.

Code: `src/backtest/state.rs` (save/load), `src/backtest/dashboard.rs` (HTTP serving).
