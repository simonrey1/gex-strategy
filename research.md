# Research Notes

## Closest Published Work

No single paper describes our exact strategy (IV spike → compression → long equity near put wall), but it combines several well-documented mechanisms.

---

## 1. FlashAlpha GEX-Conditioned VRP Matrix (most similar)

Combines **gamma regime** (positive/negative) with **volatility risk premium** (VRP) level into a 4-cell matrix.

**Cell A ("Premium Paradise")**: Positive gamma + high VRP — occurs 2–5 days after a vol spike when IV is still elevated but dealers have stabilized. Vanna-driven flows create a virtuous cycle: vol drops → dealer deltas shift → dealers buy shares → market rises → vol drops further.

Our VannaFlip is essentially going long equity in Cell A conditions. The difference: they sell options (premium), we buy the underlying directly.

Source: https://flashalpha.com/articles/gex-conditioned-vrp-dealer-positioning-volatility-premium

---

## 2. Vanna Mechanics (FlashAlpha)

When IV drops post-event:
1. Dealers with positive vanna see deltas decrease
2. Dealers become over-hedged (excess short stock)
3. Dealers **buy shares** to rebalance → mechanical bid
4. Rally further suppresses vol → reinforcing cycle

This "vol-compression rally" is most powerful in post-event environments where IV crush is sharpest.

Key insight: **Vanna is more predictive than gamma** for post-spike rallies. Dealers are more sensitive to IV changes than to price changes (Volland whitepaper, Dec 2023).

Source: https://flashalpha.com/articles/vanna-charm-second-order-greeks-guide

---

## 3. Academic Papers

| Paper | SSRN | Finding | Relevance |
|---|---|---|---|
| Gamma Fragility (Barbon & Buraschi) | 3725454 | Dealer gamma imbalances create intraday momentum/reversal; strongest in illiquid names | Validates gamma walls as mechanical price support |
| Option Gamma and Stock Returns | 4256259 | Low net gamma stocks outperform by ~10%/yr | Gamma regime predicts returns |
| Option Expected Hedging Demand | 4729672 | Delta-hedging demand predicts cross-sectional returns for 5 days then reverses | Our 50-bar entry window aligns with this 5-day effect |
| Volland Whitepaper | — | Dealers more sensitive to IV changes than price; vanna > gamma for flow prediction | Supports vanna-flip thesis over pure gamma |
| Impact of Option Dealer Flows | 4669282 | Dealer hedging drives daily and intraday returns | General validation |

---

## 4. SpotGamma Put Wall Statistics (900+ sessions)

- Put wall **held as support 89%** of sessions, breached only 8%
- After breach: +14bps (1d), +39bps (10d) forward returns (mean-reversion bounce)
- Put wall shifting higher → 0.62 correlation with equity gains over next 2–3 days
- Call wall held 83% of sessions, breached 17%

Directly validates our `near_put_wall` spike condition.

Source: https://spotgamma.com/option-wall-stats/

---

## 5. VIX Mean Reversion Timing

- VIX > 30 → 78.4% probability lower within 10 trading days
- Average reversion time from spike: **7.2 trading days**
- Optimal entry zone: VIX 15–20 (68.3% win rate on bullish plays)
- VRP widens after spikes: 7.1pts when VIX > 30 vs 1.4pts when VIX 10–15

Source: https://www.ipresage.com/research/vix-mean-reversion

---

## Ideas to Explore

### A. Vanna exposure as entry filter
We compute gamma walls but not aggregate vanna. A **net vanna sign** or magnitude condition could improve entry quality — positive vanna means vol compression mechanically pushes price up.

### B. VRP z-score as spike quality metric
Instead of just IV spike > baseline, compute **VRP z-score** (IV vs trailing realized vol). A spike where IV is high but RV is also high is less profitable than IV overshooting RV.

### C. Gamma flip level as exit/trail anchor
The gamma flip is a critical regime boundary. If price crosses below the flip, dealer flows reverse. Could be a more adaptive exit than fixed ATR-based stops.

### D. Put wall shift direction
SpotGamma: 0.62 correlation between put wall rising and subsequent equity gains. We check proximity to put wall, but not whether the **put wall is moving higher** (bullish signal) at spike time.

### E. Charm exposure on expiry days
0DTE/weekly charm flows are massive. Entry quality may differ on expiry vs non-expiry days. We don't currently factor in DTE of nearby option concentrations.
