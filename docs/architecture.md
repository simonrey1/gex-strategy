# Architecture

## Data flow

```
ThetaData (15-min all_greeks, ±25% strikes)
  → gex_builder.rs → GEX walls (narrow/wide/weekly), EWLS accel, VEX, dealer delta
  → parquet cache (data/unified/)
  → strategy pipeline (signals.rs → entries/ → engine.rs)
  → runner.rs → Broker (IBKR TWS API)
```

## Core types (`types.rs`)

`OhlcBar`, `GexProfile` (walls + net GEX + accel + IV), `WallLevel` (strike + gamma_oi), `Signal`, `TradeSignal`.

Config: `StrategyConfig` in `config/strategy.rs`.

## GEX walls

All from 15-min `all_greeks` data (±25% strikes, all expirations):
- **Narrow** = highest γ×OI near spot. Jumpy.
- **Wide** = >3% OTM, structural. Used for wall-trailing SL.
- **Weekly** = Friday-expiry only. Tighter to spot than wide.

## Options pipeline

Single-tier fetch: `raw_options_wide_v{N}.json.gz` (~230 KB/day). 4 concurrent expirations.

Validation: `option_row_valid()`: gamma > 0, OI > 0, 0.01 < IV ≤ 5.0, strikes ±25%.

## Cache

```
data/unified/{TICKER}/
  bars_1m.parquet          # 1-min OHLCV (zstd)
  gex_15m_v{V}.parquet     # GEX profiles (zstd)
```

Parquet files tracked via Git LFS (~315 MB total). Bump `V` → recompute GEX. Bump `N` → re-download.

## TypeScript bindings

Auto-generated via ts-rs. Don't hand-edit `bindings/shared/generated/*.ts`.

`shared/types.ts` wraps with `Numberify<T>`. Dashboard imports from `@shared/types`.

Rebuild after TS changes: `cd dashboard && npm run build`.
