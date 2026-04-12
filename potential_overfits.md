# Parameter Robustness Report

Commit: `dd13d9e` | Generated: 2026-04-12 17:44

Regenerate: `cargo run --release --bin backtest -- --overfit-report`

## Baseline

| Period | Sharpe | Sortino | MDD | Net | Trades |
| --- | ---: | ---: | ---: | ---: | ---: |
| FULL (2018–2026) | 2.82 | 4.94 | 9.0% | $122146 | 493 |
| H1 (2018–2022) | 2.95 | 5.46 | 7.6% | $25642 | 231 |
| H2 (2022–2026) | 2.66 | 4.46 | 9.0% | $25412 | 259 |
| P1 (2018–2020) | 2.44 | 4.21 | 7.6% | $5810 | 101 |
| P2 (2020–2022) | 3.28 | 6.20 | 7.3% | $11667 | 129 |
| P3 (2022–2024) | 2.70 | 4.47 | 9.0% | $9283 | 132 |
| P4 (2024–2026) | 2.57 | 4.34 | 7.7% | $7689 | 126 |

## Half-Period Parameter Sweep

H1 = 2018–2022, H2 = 2022–2026. Params were tuned on the full period, so this is a stability check, not a true holdout. Each param swept independently.

| Param (default) | H1-opt | H1 Sharpe | H2 Sharpe | Transfer |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 4.5 | 2.95 | 2.66 | Stable |
| `sl_tsi_adapt` (1.62) | 1.50 | 2.98 | 2.69 | Stable |
| `hurst_exhaust_threshold` (0.45) | 0.45 | 2.95 | 2.66 | Stable |
| `vf_max_wall_spread_atr` (10) | 10 | 2.95 | 2.66 | Stable |
| `vf_dead_zone_width` (33.33) | 33.33 | 2.95 | 2.66 | Stable |
| `vf_cw_scw_persist_bars` (5) | 10 | 2.99 | 2.48 | Stable |
| `vf_min_pw_spw_atr` (-1.5) | -1.5 | 2.95 | 2.66 | Stable |
| `vf_min_atr_pct` (0.3) | 0.2 | 3.00 | 2.55 | Stable |
| `vf_max_atr_pct` (0.5) | 0.5 | 2.95 | 2.66 | Stable |
| `vf_max_slow_atr_pct` (0.55) | 0.55 | 2.95 | 2.66 | Stable |
| `spike_min_atr_pct` (0.3) | 0.3 | 2.95 | 2.66 | Stable |
| `iv_spike_mult` (3.5) | 3.5 | 2.95 | 2.66 | Stable |
| `vf_compress_tsi_max` (0) | 0 | 2.95 | 2.66 | Stable |
| `vf_max_gex_norm` (2) | 2 | 3.00 | 2.63 | Stable |
| `wall_trail_cushion_atr` (3) | 3 | 2.95 | 2.66 | Stable |
| `tp_proximity_trigger` (0.85) | 0.85 | 2.95 | 2.66 | Stable |
| `spread_smooth_halflife` (25) | 20 | 2.97 | 2.42 | Stable |
| `wb_min_zone_score` (1.5) | 1.5 | 2.95 | 2.66 | Stable |
| `wb_min_wall_spread_atr` (5) | 3 | 2.98 | 2.62 | Stable |
| `wb_tp_spread_mult` (0.95) | 0.95 | 2.95 | 2.66 | Stable |

## 4-Period Stability

P1=2018–2020, P2=2020–2022, P3=2022–2024, P4=2024–2026.
Each param swept independently. **Bold** = peak at default. Δ = Sharpe gain over default.

| Param (default) | FULL | P1 | P2 | P3 | P4 | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `exit_width_atr` (4.5) | **4.5** | **4.5** | **4.5** | **4.5** | **4.5** | ✅ All default |
| `sl_tsi_adapt` (1.62) | **1.62** | 1.75 (Δ+0.03) | 1.50 (Δ+0.10) | 1.50 (Δ+0.11) | 1.00 (Δ+0.08) | ⚠️ 4/5 diverge, max Δ+0.11 |
| `hurst_exhaust_threshold` (0.45) | **0.45** | **0.45** | **0.45** | **0.45** | 0.40 (Δ+0.01) | ✅ 1/5 diverge, max Δ+0.01 |
| `vf_max_wall_spread_atr` (10) | **10** | 8 (Δ+0.05) | 12 (Δ+0.09) | **10** | **10** | ✅ 2/5 diverge, max Δ+0.09 |
| `vf_dead_zone_width` (33.33) | **33.33** | **33.33** | 40.00 (Δ+0.14) | **33.33** | 35.00 (Δ+0.02) | ⚠️ 2/5 diverge, max Δ+0.14 |
| `vf_cw_scw_persist_bars` (5) | **5** | 6 (Δ+0.05) | 10 (Δ+0.17) | **5** | **5** | ⚠️ 2/5 diverge, max Δ+0.17 |
| `vf_min_pw_spw_atr` (-1.5) | **-1.5** | **-1.5** | -4.0 (Δ+0.30) | -1.0 (Δ+0.05) | 0.0 (Δ+0.25) | ⚠️ 3/5 diverge, max Δ+0.30 |
| `vf_min_atr_pct` (0.3) | **0.3** | 0.2 (Δ+0.12) | **0.3** | **0.3** | **0.3** | ⚠️ 1/5 diverge, max Δ+0.12 |
| `vf_max_atr_pct` (0.5) | **0.5** | **0.5** | **0.5** | **0.5** | **0.5** | ✅ All default |
| `vf_max_slow_atr_pct` (0.55) | **0.55** | **0.55** | 0.00 (Δ+0.06) | **0.55** | **0.55** | ✅ 1/5 diverge, max Δ+0.06 |
| `spike_min_atr_pct` (0.3) | **0.3** | **0.3** | **0.3** | **0.3** | **0.3** | ✅ All default |
| `iv_spike_mult` (3.5) | **3.5** | **3.5** | **3.5** | **3.5** | **3.5** | ✅ All default |
| `vf_compress_tsi_max` (0) | **0** | **0** | 5 (Δ+0.05) | **0** | **0** | ✅ 1/5 diverge, max Δ+0.05 |
| `vf_max_gex_norm` (2) | 2 (Δ+0.01) | **2** | 2 (Δ+0.05) | **2** | **2** | ✅ 2/5 diverge, max Δ+0.05 |
| `wall_trail_cushion_atr` (3) | **3** | 2 (Δ+0.15) | **3** | **3** | **3** | ⚠️ 1/5 diverge, max Δ+0.15 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.95 (Δ+0.07) | **0.85** | **0.85** | **0.85** | ✅ 1/5 diverge, max Δ+0.07 |
| `spread_smooth_halflife` (25) | **25** | 20 (Δ+0.01) | 20 (Δ+0.03) | **25** | **25** | ✅ 2/5 diverge, max Δ+0.03 |
| `wb_min_zone_score` (1.5) | **1.5** | **1.5** | **1.5** | **1.5** | **1.5** | ✅ All default |
| `wb_min_wall_spread_atr` (5) | **5** | **5** | 3 (Δ+0.05) | **5** | 3 (Δ+0.03) | ✅ 2/5 diverge, max Δ+0.05 |
| `wb_tp_spread_mult` (0.95) | **0.95** | **0.95** | 2.25 (Δ+0.08) | **0.95** | 2.25 (Δ+0.37) | ⚠️ 2/5 diverge, max Δ+0.37 |

## Cross-Ticker Alignment (FULL Period)

Per-ticker solo Sharpe peak vs portfolio default. Sorted by avg Δ (highest first).

| Param (default) | At default | Diverged | Avg Δ | Max Δ | Worst offenders |
| --- | ---: | ---: | ---: | ---: | --- |
| `wb_tp_spread_mult` (0.95) | 0/15 | 1/15 | 0.28 | 0.28 | JPM: wants 2.50 (Sharpe +0.28) |
| `spike_min_atr_pct` (0.3) | 5/15 | 10/15 | 0.11 | 0.21 | COST: wants 0.1 (Sharpe +0.21)<br>HD: wants 0.1 (Sharpe +0.20)<br>MCD: wants 0.5 (Sharpe +0.18)<br>NRG: wants 0.5 (Sharpe +0.14) |
| `vf_max_atr_pct` (0.5) | 7/15 | 8/15 | 0.09 | 0.25 | NRG: wants 0.4 (Sharpe +0.25)<br>GS: wants 0.0 (Sharpe +0.19)<br>JPM: wants 0.6 (Sharpe +0.11)<br>KO: wants 0.4 (Sharpe +0.07) |
| `iv_spike_mult` (3.5) | 6/15 | 9/15 | 0.09 | 0.18 | WMT: wants 3.0 (Sharpe +0.18)<br>JPM: wants 5.0 (Sharpe +0.17)<br>KO: wants 4.0 (Sharpe +0.14)<br>NRG: wants 5.0 (Sharpe +0.10) |
| `exit_width_atr` (4.5) | 4/15 | 11/15 | 0.09 | 0.19 | SO: wants 6.0 (Sharpe +0.19)<br>MSFT: wants 5.0 (Sharpe +0.15)<br>NRG: wants 3.5 (Sharpe +0.15)<br>GOOG: wants 5.5 (Sharpe +0.11) |
| `hurst_exhaust_threshold` (0.45) | 3/15 | 12/15 | 0.07 | 0.35 | NRG: wants 0.60 (Sharpe +0.35)<br>DIS: wants 0.55 (Sharpe +0.17)<br>HD: wants 0.50 (Sharpe +0.09)<br>MS: wants 0.40 (Sharpe +0.07) |
| `vf_dead_zone_width` (33.33) | 4/15 | 11/15 | 0.07 | 0.11 | NRG: wants 40.00 (Sharpe +0.11)<br>COST: wants 40.00 (Sharpe +0.11)<br>AAPL: wants 40.00 (Sharpe +0.10)<br>GOOG: wants 15.00 (Sharpe +0.09) |
| `vf_max_slow_atr_pct` (0.55) | 3/15 | 12/15 | 0.07 | 0.15 | MSFT: wants 0.65 (Sharpe +0.15)<br>NRG: wants 0.50 (Sharpe +0.12)<br>MS: wants 0.45 (Sharpe +0.11)<br>JPM: wants 0.40 (Sharpe +0.09) |
| `vf_cw_scw_persist_bars` (5) | 4/15 | 11/15 | 0.06 | 0.18 | NRG: wants 8 (Sharpe +0.18)<br>HD: wants 6 (Sharpe +0.09)<br>WMT: wants 4 (Sharpe +0.07)<br>KO: wants 2 (Sharpe +0.06) |
| `sl_tsi_adapt` (1.62) | 5/15 | 10/15 | 0.06 | 0.20 | SO: wants 4.00 (Sharpe +0.20)<br>COST: wants 1.00 (Sharpe +0.09)<br>GOOG: wants 1.00 (Sharpe +0.07)<br>MCD: wants 2.50 (Sharpe +0.06) |
| `vf_max_wall_spread_atr` (10) | 7/15 | 8/15 | 0.06 | 0.10 | COST: wants 8 (Sharpe +0.10)<br>NRG: wants 12 (Sharpe +0.10)<br>WMT: wants 12 (Sharpe +0.09)<br>MSFT: wants 7 (Sharpe +0.05) |
| `vf_min_pw_spw_atr` (-1.5) | 3/15 | 12/15 | 0.05 | 0.12 | COST: wants -2.0 (Sharpe +0.12)<br>HD: wants 0.0 (Sharpe +0.11)<br>MSFT: wants -0.5 (Sharpe +0.07)<br>GOOG: wants 0.0 (Sharpe +0.07) |
| `vf_min_atr_pct` (0.3) | 4/15 | 11/15 | 0.04 | 0.14 | AAPL: wants 0.2 (Sharpe +0.14)<br>NRG: wants 0.2 (Sharpe +0.07)<br>DIS: wants 0.3 (Sharpe +0.07)<br>CAT: wants 0.3 (Sharpe +0.04) |
| `spread_smooth_halflife` (25) | 6/15 | 9/15 | 0.04 | 0.09 | GS: wants 15 (Sharpe +0.09)<br>MSFT: wants 10 (Sharpe +0.05)<br>JPM: wants 30 (Sharpe +0.04)<br>HD: wants 10 (Sharpe +0.04) |
| `vf_compress_tsi_max` (0) | 14/15 | 1/15 | 0.04 | 0.04 | JPM: wants 10 (Sharpe +0.04) |
| `wall_trail_cushion_atr` (3) | 4/15 | 11/15 | 0.03 | 0.10 | NRG: wants 2 (Sharpe +0.10)<br>JPM: wants 2 (Sharpe +0.07)<br>MSFT: wants 4 (Sharpe +0.04)<br>GS: wants 2 (Sharpe +0.04) |
| `vf_max_gex_norm` (2) | 10/15 | 5/15 | 0.03 | 0.08 | KO: wants 2 (Sharpe +0.08)<br>AAPL: wants 2 (Sharpe +0.03)<br>CAT: wants 2 (Sharpe +0.01)<br>WMT: wants 2 (Sharpe +0.01) |
| `wb_min_wall_spread_atr` (5) | 0/15 | 1/15 | 0.03 | 0.03 | JPM: wants 3 (Sharpe +0.03) |
| `tp_proximity_trigger` (0.85) | 7/15 | 8/15 | 0.01 | 0.02 | HD: wants 0.75 (Sharpe +0.02)<br>DIS: wants 0.80 (Sharpe +0.02)<br>GS: wants 0.80 (Sharpe +0.01)<br>AAPL: wants 0.80 (Sharpe +0.01) |
| `wb_min_zone_score` (1.5) | 1/15 | 0/15 | 0.00 | 0.00 |  |

## Per-Ticker Sweep (FULL Period)

### AAPL

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 5.0 | 0.94 | +0.09 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | 1.00 | 0.87 | +0.02 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | **0.45** | 0.85 | +0.00 | ✅ |
| `vf_max_wall_spread_atr` (10) | **10** | 0.84 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | 40.00 | 0.94 | +0.10 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | **5** | 0.84 | +0.00 | ✅ |
| `vf_min_pw_spw_atr` (-1.5) | -2.0 | 0.86 | +0.02 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.98 | +0.14 | ⚠️ |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.85 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | 0.60 | 0.85 | +0.01 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | **0.3** | 0.84 | +0.00 | ✅ |
| `iv_spike_mult` (3.5) | **3.5** | 0.84 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.84 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | 2 | 0.88 | +0.03 | ✅ Δ<0.10 |
| `wall_trail_cushion_atr` (3) | **3** | 0.85 | +0.00 | ✅ |
| `tp_proximity_trigger` (0.85) | 0.80 | 0.86 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | **25** | 0.84 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### GOOG

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 5.5 | 0.92 | +0.11 | ⚠️ |
| `sl_tsi_adapt` (1.62) | 1.00 | 0.89 | +0.07 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.60 | 0.84 | +0.03 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | **10** | 0.81 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | 15.00 | 0.91 | +0.09 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 10 | 0.87 | +0.06 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | 0.0 | 0.88 | +0.07 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.82 | +0.01 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.81 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | 0.60 | 0.84 | +0.02 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.1 | 0.92 | +0.11 | ⚠️ |
| `iv_spike_mult` (3.5) | 3.0 | 0.91 | +0.10 | ✅ Δ<0.10 |
| `vf_compress_tsi_max` (0) | **0** | 0.81 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.82 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.83 | +0.02 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | 0.75 | 0.82 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | **25** | 0.81 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### MSFT

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 5.0 | 0.76 | +0.15 | ⚠️ |
| `sl_tsi_adapt` (1.62) | 1.00 | 0.66 | +0.05 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.55 | 0.67 | +0.06 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | 7 | 0.66 | +0.05 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | 40.00 | 0.61 | +0.01 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 8 | 0.66 | +0.06 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -0.5 | 0.68 | +0.07 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.63 | +0.02 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.61 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | 0.65 | 0.75 | +0.15 | ⚠️ |
| `spike_min_atr_pct` (0.3) | 0.2 | 0.66 | +0.06 | ✅ Δ<0.10 |
| `iv_spike_mult` (3.5) | **3.5** | 0.61 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.61 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | 2 | 0.62 | +0.01 | ✅ Δ<0.10 |
| `wall_trail_cushion_atr` (3) | 4 | 0.65 | +0.04 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.61 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | 10 | 0.65 | +0.05 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### JPM

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 5.5 | 0.51 | +0.02 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | **1.62** | 0.49 | +0.00 | ✅ |
| `hurst_exhaust_threshold` (0.45) | **0.45** | 0.49 | +0.00 | ✅ |
| `vf_max_wall_spread_atr` (10) | 20 | 0.51 | +0.02 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | 25.00 | 0.56 | +0.07 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 8 | 0.54 | +0.05 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -1.0 | 0.51 | +0.02 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.52 | +0.03 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.6 | 0.60 | +0.11 | ⚠️ |
| `vf_max_slow_atr_pct` (0.55) | 0.40 | 0.58 | +0.09 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.5 | 0.54 | +0.05 | ✅ Δ<0.10 |
| `iv_spike_mult` (3.5) | 5.0 | 0.66 | +0.17 | ⚠️ |
| `vf_compress_tsi_max` (0) | 10 | 0.53 | +0.04 | ✅ Δ<0.10 |
| `vf_max_gex_norm` (2) | **2** | 0.49 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.56 | +0.07 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.50 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | 30 | 0.53 | +0.04 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | **1.5** | 0.49 | +0.00 | ✅ |
| `wb_min_wall_spread_atr` (5) | 3 | 0.52 | +0.03 | ✅ Δ<0.10 |
| `wb_tp_spread_mult` (0.95) | 2.50 | 0.77 | +0.28 | ⚠️ |

### GS

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 6.0 | 0.68 | +0.10 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | **1.62** | 0.59 | +0.00 | ✅ |
| `hurst_exhaust_threshold` (0.45) | 0.40 | 0.61 | +0.03 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | **10** | 0.59 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | **33.33** | 0.59 | +0.00 | ✅ |
| `vf_cw_scw_persist_bars` (5) | 4 | 0.63 | +0.04 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -2.0 | 0.62 | +0.04 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | **0.3** | 0.59 | +0.00 | ✅ |
| `vf_max_atr_pct` (0.5) | 0.0 | 0.77 | +0.19 | ⚠️ |
| `vf_max_slow_atr_pct` (0.55) | 0.60 | 0.68 | +0.09 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.5 | 0.66 | +0.07 | ✅ Δ<0.10 |
| `iv_spike_mult` (3.5) | 4.0 | 0.59 | +0.01 | ✅ Δ<0.10 |
| `vf_compress_tsi_max` (0) | **0** | 0.59 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.59 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.62 | +0.04 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | 0.80 | 0.60 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | 15 | 0.68 | +0.09 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### WMT

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 5.5 | 0.96 | +0.05 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | 1.00 | 0.97 | +0.06 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.40 | 0.93 | +0.01 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | 12 | 1.00 | +0.09 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | 35.00 | 0.94 | +0.03 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 4 | 0.98 | +0.07 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | **-1.5** | 0.91 | +0.00 | ✅ |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.92 | +0.01 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.6 | 0.94 | +0.03 | ✅ Δ<0.10 |
| `vf_max_slow_atr_pct` (0.55) | 0.40 | 0.92 | +0.01 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | **0.3** | 0.91 | +0.00 | ✅ |
| `iv_spike_mult` (3.5) | 3.0 | 1.10 | +0.18 | ⚠️ |
| `vf_compress_tsi_max` (0) | **0** | 0.91 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | 2 | 0.93 | +0.01 | ✅ Δ<0.10 |
| `wall_trail_cushion_atr` (3) | 4 | 0.92 | +0.01 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.91 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | **25** | 0.91 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### HD

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | **4.5** | 0.64 | +0.00 | ✅ |
| `sl_tsi_adapt` (1.62) | **1.62** | 0.64 | +0.00 | ✅ |
| `hurst_exhaust_threshold` (0.45) | 0.50 | 0.73 | +0.09 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | 20 | 0.67 | +0.03 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | **33.33** | 0.64 | +0.00 | ✅ |
| `vf_cw_scw_persist_bars` (5) | 6 | 0.73 | +0.09 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | 0.0 | 0.75 | +0.11 | ⚠️ |
| `vf_min_atr_pct` (0.3) | **0.3** | 0.64 | +0.00 | ✅ |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.64 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | **0.55** | 0.64 | +0.00 | ✅ |
| `spike_min_atr_pct` (0.3) | 0.1 | 0.84 | +0.20 | ⚠️ |
| `iv_spike_mult` (3.5) | 2.0 | 0.67 | +0.03 | ✅ Δ<0.10 |
| `vf_compress_tsi_max` (0) | **0** | 0.64 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.64 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.67 | +0.03 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | 0.75 | 0.66 | +0.02 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | 10 | 0.68 | +0.04 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### DIS

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | **4.5** | 0.40 | +0.00 | ✅ |
| `sl_tsi_adapt` (1.62) | **1.62** | 0.40 | +0.00 | ✅ |
| `hurst_exhaust_threshold` (0.45) | 0.55 | 0.57 | +0.17 | ⚠️ |
| `vf_max_wall_spread_atr` (10) | **10** | 0.40 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | 30.00 | 0.47 | +0.07 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 4 | 0.41 | +0.01 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | 0.0 | 0.41 | +0.01 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.3 | 0.47 | +0.07 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.40 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | 0.60 | 0.48 | +0.08 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.5 | 0.42 | +0.02 | ✅ Δ<0.10 |
| `iv_spike_mult` (3.5) | **3.5** | 0.40 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.40 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.40 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | **3** | 0.40 | +0.00 | ✅ |
| `tp_proximity_trigger` (0.85) | 0.80 | 0.42 | +0.02 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | 10 | 0.43 | +0.03 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### KO

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | **4.5** | 0.67 | +0.00 | ✅ |
| `sl_tsi_adapt` (1.62) | 0.00 | 0.67 | +0.01 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.50 | 0.67 | +0.01 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | **10** | 0.67 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | 15.00 | 0.71 | +0.04 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 2 | 0.73 | +0.06 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -4.0 | 0.69 | +0.02 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.3 | 0.71 | +0.04 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.4 | 0.74 | +0.07 | ✅ Δ<0.10 |
| `vf_max_slow_atr_pct` (0.55) | **0.55** | 0.67 | +0.00 | ✅ |
| `spike_min_atr_pct` (0.3) | **0.3** | 0.67 | +0.00 | ✅ |
| `iv_spike_mult` (3.5) | 4.0 | 0.80 | +0.14 | ⚠️ |
| `vf_compress_tsi_max` (0) | **0** | 0.67 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | 2 | 0.75 | +0.08 | ✅ Δ<0.10 |
| `wall_trail_cushion_atr` (3) | **3** | 0.67 | +0.00 | ✅ |
| `tp_proximity_trigger` (0.85) | 0.75 | 0.67 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | **25** | 0.67 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### CAT

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 3.5 | 0.79 | +0.02 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | 3.00 | 0.80 | +0.02 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | **0.45** | 0.77 | +0.00 | ✅ |
| `vf_max_wall_spread_atr` (10) | 8 | 0.82 | +0.05 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | **33.33** | 0.77 | +0.00 | ✅ |
| `vf_cw_scw_persist_bars` (5) | **5** | 0.77 | +0.00 | ✅ |
| `vf_min_pw_spw_atr` (-1.5) | **-1.5** | 0.77 | +0.00 | ✅ |
| `vf_min_atr_pct` (0.3) | 0.3 | 0.82 | +0.04 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.6 | 0.82 | +0.05 | ✅ Δ<0.10 |
| `vf_max_slow_atr_pct` (0.55) | 0.60 | 0.80 | +0.03 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | **0.3** | 0.77 | +0.00 | ✅ |
| `iv_spike_mult` (3.5) | **3.5** | 0.77 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.77 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | 2 | 0.79 | +0.01 | ✅ Δ<0.10 |
| `wall_trail_cushion_atr` (3) | 2 | 0.78 | +0.01 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.77 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | 20 | 0.80 | +0.03 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### MS

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 3.0 | 0.39 | +0.05 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | 1.75 | 0.36 | +0.02 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.40 | 0.41 | +0.07 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | **10** | 0.34 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | **33.33** | 0.34 | +0.00 | ✅ |
| `vf_cw_scw_persist_bars` (5) | **5** | 0.34 | +0.00 | ✅ |
| `vf_min_pw_spw_atr` (-1.5) | **-1.5** | 0.34 | +0.00 | ✅ |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.35 | +0.01 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.0 | 0.36 | +0.02 | ✅ Δ<0.10 |
| `vf_max_slow_atr_pct` (0.55) | 0.45 | 0.45 | +0.11 | ⚠️ |
| `spike_min_atr_pct` (0.3) | **0.3** | 0.34 | +0.00 | ✅ |
| `iv_spike_mult` (3.5) | **3.5** | 0.34 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.34 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.34 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.36 | +0.01 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.35 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | 20 | 0.36 | +0.02 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### NRG

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 3.5 | 0.32 | +0.15 | ⚠️ |
| `sl_tsi_adapt` (1.62) | **1.62** | 0.17 | +0.00 | ✅ |
| `hurst_exhaust_threshold` (0.45) | 0.60 | 0.52 | +0.35 | ⚠️ |
| `vf_max_wall_spread_atr` (10) | 12 | 0.27 | +0.10 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | 40.00 | 0.29 | +0.11 | ⚠️ |
| `vf_cw_scw_persist_bars` (5) | 8 | 0.35 | +0.18 | ⚠️ |
| `vf_min_pw_spw_atr` (-1.5) | 0.0 | 0.23 | +0.06 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.24 | +0.07 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.4 | 0.42 | +0.25 | ⚠️ |
| `vf_max_slow_atr_pct` (0.55) | 0.50 | 0.29 | +0.12 | ⚠️ |
| `spike_min_atr_pct` (0.3) | 0.5 | 0.31 | +0.14 | ⚠️ |
| `iv_spike_mult` (3.5) | 5.0 | 0.27 | +0.10 | ⚠️ |
| `vf_compress_tsi_max` (0) | **0** | 0.17 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.18 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.27 | +0.10 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.17 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | **25** | 0.17 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### SO

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 6.0 | 0.79 | +0.19 | ⚠️ |
| `sl_tsi_adapt` (1.62) | 4.00 | 0.81 | +0.20 | ⚠️ |
| `hurst_exhaust_threshold` (0.45) | 0.40 | 0.62 | +0.02 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | 20 | 0.62 | +0.01 | ✅ Δ<0.10 |
| `vf_dead_zone_width` (33.33) | 15.00 | 0.63 | +0.02 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 12 | 0.66 | +0.05 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -4.0 | 0.65 | +0.05 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | **0.3** | 0.61 | +0.00 | ✅ |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.61 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | 0.50 | 0.65 | +0.05 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.4 | 0.66 | +0.05 | ✅ Δ<0.10 |
| `iv_spike_mult` (3.5) | 4.0 | 0.70 | +0.09 | ✅ Δ<0.10 |
| `vf_compress_tsi_max` (0) | **0** | 0.61 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.61 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | **3** | 0.61 | +0.00 | ✅ |
| `tp_proximity_trigger` (0.85) | 0.95 | 0.61 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | 35 | 0.62 | +0.01 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### MCD

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | 4.0 | 0.71 | +0.05 | ✅ Δ<0.10 |
| `sl_tsi_adapt` (1.62) | 2.50 | 0.73 | +0.06 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.50 | 0.68 | +0.01 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | **10** | 0.66 | +0.00 | ✅ |
| `vf_dead_zone_width` (33.33) | 15.00 | 0.75 | +0.08 | ✅ Δ<0.10 |
| `vf_cw_scw_persist_bars` (5) | 12 | 0.69 | +0.03 | ✅ Δ<0.10 |
| `vf_min_pw_spw_atr` (-1.5) | -0.5 | 0.69 | +0.03 | ✅ Δ<0.10 |
| `vf_min_atr_pct` (0.3) | **0.3** | 0.66 | +0.00 | ✅ |
| `vf_max_atr_pct` (0.5) | **0.5** | 0.66 | +0.00 | ✅ |
| `vf_max_slow_atr_pct` (0.55) | **0.55** | 0.66 | +0.00 | ✅ |
| `spike_min_atr_pct` (0.3) | 0.5 | 0.84 | +0.18 | ⚠️ |
| `iv_spike_mult` (3.5) | **3.5** | 0.66 | +0.00 | ✅ |
| `vf_compress_tsi_max` (0) | **0** | 0.66 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.66 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.68 | +0.02 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | **0.85** | 0.66 | +0.00 | ✅ |
| `spread_smooth_halflife` (25) | **25** | 0.66 | +0.00 | ✅ |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

### COST

| Param (default) | Peak | Sharpe | Δ | Status |
| --- | ---: | ---: | ---: | --- |
| `exit_width_atr` (4.5) | **4.5** | 0.72 | +0.00 | ✅ |
| `sl_tsi_adapt` (1.62) | 1.00 | 0.81 | +0.09 | ✅ Δ<0.10 |
| `hurst_exhaust_threshold` (0.45) | 0.55 | 0.78 | +0.06 | ✅ Δ<0.10 |
| `vf_max_wall_spread_atr` (10) | 8 | 0.83 | +0.10 | ⚠️ |
| `vf_dead_zone_width` (33.33) | 40.00 | 0.83 | +0.11 | ⚠️ |
| `vf_cw_scw_persist_bars` (5) | **5** | 0.72 | +0.00 | ✅ |
| `vf_min_pw_spw_atr` (-1.5) | -2.0 | 0.84 | +0.12 | ⚠️ |
| `vf_min_atr_pct` (0.3) | 0.2 | 0.75 | +0.02 | ✅ Δ<0.10 |
| `vf_max_atr_pct` (0.5) | 0.5 | 0.78 | +0.05 | ✅ Δ<0.10 |
| `vf_max_slow_atr_pct` (0.55) | 0.00 | 0.76 | +0.04 | ✅ Δ<0.10 |
| `spike_min_atr_pct` (0.3) | 0.1 | 0.93 | +0.21 | ⚠️ |
| `iv_spike_mult` (3.5) | 2.0 | 0.76 | +0.04 | ✅ Δ<0.10 |
| `vf_compress_tsi_max` (0) | **0** | 0.72 | +0.00 | ✅ |
| `vf_max_gex_norm` (2) | **2** | 0.72 | +0.00 | ✅ |
| `wall_trail_cushion_atr` (3) | 2 | 0.75 | +0.03 | ✅ Δ<0.10 |
| `tp_proximity_trigger` (0.85) | 0.80 | 0.73 | +0.01 | ✅ Δ<0.10 |
| `spread_smooth_halflife` (25) | 10 | 0.76 | +0.04 | ✅ Δ<0.10 |
| `wb_min_zone_score` (1.5) | — | — | — | — |
| `wb_min_wall_spread_atr` (5) | — | — | — | — |
| `wb_tp_spread_mult` (0.95) | — | — | — | — |

