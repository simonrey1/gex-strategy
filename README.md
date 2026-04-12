# gex-strategy

Long-only GEX strategy for US equities. Trades dealer gamma/vanna positioning from options data, enters on IV spike → compression reversals, exits via bracket SL/TP + wall-trailing stop. Rust backtester + live engine with IBKR bracket orders and React dashboard.

**Not financial advice.** Educational/research only. Trading = risk. Past performance ≠ future results.

> **Want to help?** I'm looking for people to stress-test this: find lookahead bias, break the [half-period stability](potential_overfits.md), challenge execution assumptions, or improve [entry gate logic](docs/strategy.md). Run `--overfit-report` or `--missed-entries` and tell me what's wrong. See the [full ask](#want-to-help-1) below.

---

## Quick start

Need [Rust](https://rustup.rs) and [Git LFS](https://git-lfs.com/).

```bash
git clone https://github.com/simonrey1/gex-strategy.git
cd gex-strategy
git lfs pull  # ~315 MB parquet data (8 years, 15 tickers)
```

```bash
cargo run --release --bin backtest                           # full portfolio
cargo run --release --bin backtest -- --ticker AAPL -p 8080  # single ticker + dashboard
```

What you get (~30 seconds after first compile):

```
[backtest] === RESULTS: AAPL,GOOG,MSFT,JPM,GS,WMT,HD,DIS,KO,CAT,MS,NRG,SO,MCD,COST ===
  Period:       2018-01-15 -> 2026-04-01
  Bars:         50766 (15-min)
  Trades:       493
  Win/Loss:     217W / 276L  (44.0%)
  Gross PnL:    $132607.43
  Commission:   -$1031.21 (~$2.09/trade)
  Slippage:     -$9430.20
  Net PnL:      $122146.02
  ┌───────────────────────────────────┐
  │  Strategy:   +1221.46% on $10000  │
  │  Buy & Hold: +235.59%             │
  │  Alpha:      +985.87%             │
  └───────────────────────────────────┘
  Profit Factor: 2.99
  Max Drawdown: $6129.31 (9.03%)
  Sharpe:       2.82
  Sortino:      4.94
  Calmar:       3.93
  CAGR:         35.47%
  Ulcer Index:  0.0192
  Expectancy:   $247.76/trade  |  Payoff: 3.44
  Max DD Dur:   67 trading days  |  Avg Hold: 13395 min  |  Avg Capital Util: 49.2%
  Avg Win:      +6.24%  |  Avg Loss: -1.81%  |  Avg Trade: +1.73%
```

For the dashboard (needs [Node.js](https://nodejs.org/)):
```bash
cargo test --release export_bindings   # generates TypeScript bindings
cd dashboard && npm install && npm run build && cd ..
cargo run --release --bin backtest -- --ticker AAPL -p 8080
```

Opens `http://localhost:8080`: candlestick chart with GEX walls, trade markers, P&L curve, IV analysis.

No API keys, no subscriptions. Everything runs offline in ~30s.

```bash
cargo run --release --bin backtest -- --start 2022-01-01 --end 2024-01-01   # date range
cargo run --release --bin backtest -- --sweep "vf_min_atr_pct=0.20,0.25,0.30,0.35"  # param sweep
cargo run --release --bin backtest -- --overfit-report                       # stability analysis
cargo run --release --bin backtest -- --help                                 # all flags
```

---

## Thesis

Exploits **dealer hedging flows** from options GEX walls. Needs deep options liquidity: high OI creates meaningful walls that act as support/resistance. Works on the top ~20 US equities by options OI (AAPL, GOOG, MSFT, JPM, GS…). Same names dominate year after year.

## Signals

### VannaFlip (VF) — the main signal

When IV spikes — options fear surges — dealers who sold puts hedge aggressively. When the panic fades and IV compresses, those same dealers unwind: they buy back stock, creating a mechanical tailwind. The strategy detects the storm, then waits for the calm before entering.

```
1. SPIKE DETECTION — "Is there a storm?"
   ├─ IV spikes to ≥3.5× its baseline (real panic, not noise)
   ├─ Price near the put wall (near support — where hedging pressure peaks)
   └─ Minimum volatility (market active enough to trade)
   → 50-bar window (12.5h) opens.

2. ENTRY GATES — "The calm after the storm"
   Checked every 15-min bar within the window. All must pass:
   ├─ IV Compression : IV dropped to ≤50% of spike peak (fear fading)
   ├─ Max ATR% < 0.50 : price choppiness settling (not just fear headline)
   ├─ Wall structure : GEX walls present and healthy
   ├─ Trend state : TSI, momentum, dead-zone checks
   └─ 5 more structural checks (GEX norm, wall spread, PW/CW strength)
   → First bar where everything passes: buy.
```

Uses today's *peak* IV (`iv_daily_high`), not the current reading. A 9:35am flash spike keeps the window open all day — the panic already happened.

**IvCompress + AtrPct** carry most of the Sharpe — they are the "calm after the storm" filters.

**IV scan oracle** (`--missed-entries`): after spike detection, calls
`IvCompressionInputs::is_eligible()` (has spike + within 50 bars + IV exists),
then enters at every eligible bar — skipping all VF entry gates. Classifies
results as best/worst. The **b/w ratio** is affected by Stage 1 (spike
detection) and `is_eligible()`, but NOT by VF entry gates. b/w ≈ 0.28 is
invariant to spike-time conditions; improving it requires smarter filtering
in `is_eligible()` (compression-time environment).

### WallBounce (WB)

Calm-path entry near put wall via zone-dwell score. Currently JPM only.

### Exits

Bracket SL/TP + wall-trailing stop + Hurst exhaustion trailing.

## Portfolio

`max_open_positions=3` across 15 tickers. `rank_and_dedup` sorts candidates by TSI, fills available slots. ~78% of signals get skipped because slots are full: this involuntary scarcity does most of the filtering. TSI ranking itself isn't predictive (inverting sort order gives similar Sharpe).

## Regime risk

The edge depends on IV spikes. Low-vol sideways markets → fewer spikes → Sharpe roughly halves. No spike = no dealer flow to ride.

## Robustness analysis

[Overfit report](potential_overfits.md) has half-period stability, 4-period checks, per-ticker sweeps. Note: params were tuned on the full period, so H1/H2 is a stability check, not a true holdout.

Regenerate: `cargo run --release --bin backtest -- --overfit-report`

## CLI flags

| Flag | Default | |
|------|---------|---|
| `--ticker <SYM[,SYM]>` | all | Comma-separated |
| `--start/--end <DATE>` | 2018-01-01 / 2026-03-01 | |
| `-v <0-3>` | 1 | 0=sweep line, 1=summary, 2=trades, 3=rejections |
| `-p <PORT>` | off | Dashboard |
| `--overfit-report` | | Half-period + 4-period stability report |
| `--missed-entries` | | Best entries blocked by VF gates (+ dashboard mini charts) |
| `--sweep "<p=v1,v2>"` | | Single-process param sweep |

## Dashboard

Served by backtest (`-p 8080`) and live (`:3000`).

- Candlestick + GEX wall overlays + trade markers
- IV panel: ATM put IV, baseline, spike events, compression points
- IV Scan: simulates hypothetical entry at every compression bar post-spike, classifies as Best/Worst
- Trade analysis: high-runup losers (ran up but reversed to SL) + worst losses

## Live trading

Needs [Podman](https://podman.io/), ThetaData Pro, IBKR account.

```bash
kube/play.sh dev                    # IB Gateway + Theta Terminal
cargo run --release --bin live      # strategy + dashboard on :3000
```

Production: `kube/play.sh prod`. See [live trading docs](docs/live-trading.md) for VPS setup.

---

## Tests

```bash
cargo test && npm --prefix dashboard test
```

Non-regression snapshots:
```bash
INSTA_UPDATE=always INSTA_FORCE_PASS=1 cargo test --release --test portfolio_nonreg && cargo insta accept
```

## Want to help?

Params were tuned on the full 2018–2026 period. H1/H2 is a stability check, not a true holdout. I want people to find what's wrong.

- **Find lookahead bias** in entry signals, GEX wall computation, IV baseline, or exit logic
- **Break half-period stability**: find a param that pumps H1 Sharpe while H2 drops. `--overfit-report` does this but might miss subtler cases
- **Challenge execution**: are commissions ($0.005/share), slippage ($0.10), fills realistic?
- **Improve entry gates**: `--missed-entries -p 8080` shows the best entries that gates blocked, with mini charts. Better gate logic > param tweaking

Some params are sharp peaks (fragile?): `exit_width_atr`, `spike_min_atr_pct`, `vf_max_atr_pct` all drop Sharpe significantly with ±0.05 changes. Real structure or noise?

## License

[CC BY-NC-SA 4.0](LICENSE), non-commercial use only.

## Docs

| Doc | |
|-----|-|
| [Strategy](docs/strategy.md) | Signals, SL/TP, indicators, risk |
| [Architecture](docs/architecture.md) | Data flow, types, GEX walls, cache |
| [Backtest](docs/backtest.md) | Execution, commissions, chart markers |
| [Live Trading](docs/live-trading.md) | Poll loop, IBKR orders, VPS deployment |
| [Robustness](potential_overfits.md) | Half-period sweeps, per-ticker analysis |
