# Strategy

Long-only GEX on US equities. Buy the calm after the storm.

## Signals

### VannaFlip (VF)

IV spike → compression → dealer vanna buy-back.

1. **Spike**: IV daily high > baseline × `iv_spike_mult` (3.0), price crashed near/below smoothed put wall. ATR must be ≥ `spike_min_atr_pct` (0.30%) to filter weak spikes.
2. **Entry**: IV compressed below spike level, price above PW, walls valid, within 50 bars of spike. Gates:
   - **TSI dead zone** `[-w, w)` where w = `vf_dead_zone_width` (100/3): requires CW headroom and low ADX
   - **Wall spread**: `(narrow_cw - narrow_pw) / ATR ≤ 10`
   - **CW vs smoothed CW**: `(narrow_cw - smoothed_cw) / ATR ≥ 0.5`
   - **ATR% regime**: ATR/price ≥ `vf_min_atr_pct`, too calm = no vanna catalyst

### WallBounce (WB)

Price zone-dwell near put wall without an IV spike. JPM only.

## SL/TP

- **Stop-loss**: 5 ATR below entry. TSI-adaptive: widens for dip-buys, tightens for momentum.
- **Take-profit**: 30 ATR above entry. Scales by sqrt(atr_regime_ratio) in high vol.
- **Wall-trailing**: activates at 12 ATR gain, trails to highest PW minus 2.5 ATR.
- **Hurst exhaustion**: when Hurst < 0.45 for 4+ bars and gain ≥ 12 ATR, tightens to highest_close − 2 ATR.

## Risk

| | Default |
|-|---------|
| `max_open_positions` | 3 |
| `max_entries_per_day` | 10 |
| `daily_loss_limit_pct` | 15% |
| `no_entry_before_et` | 1030 |
| `no_entry_after_et` | 1500 |

## Performance

See [overfit report](../potential_overfits.md). Regenerate: `cargo run --release --bin backtest -- --overfit-report`
