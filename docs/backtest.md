# Backtest

Replays 1-min bars with simulated commissions, slippage, deferred execution.

## Flow

```
For each trading day:
  1. Load bars + GEX from parquet cache
  2. For each 1-min bar:
     a. Update indicators
     b. Tick deferred orders
     c. Check SL/TP (intra-bar OHLC)
     d. Generate signal (every 15-min GEX bar, after warmup)
     e. Queue entry/exit with execution delay
```

**Deferred execution**: 3 bars delay. Entry at `bar.open + slippage`.

**Position sizing**: `shares = floor(capital × 95% / ticker_count / entry_price)`.

**Commissions**: IBKR fixed, `max(shares × $0.005, $1.00)`. Slippage: $0.10/execution.

## Chart markers

| Marker | Color | Shape |
|--------|-------|-------|
| Entry | Blue | ▲ |
| Exit (win) | Green | ▼ |
| Exit (loss) | Red | ▼ |
| Bounce | Orange | ● |
