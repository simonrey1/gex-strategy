import type { LiveTickerStatus } from "../types";

export function IndicatorBar({ t }: { t: LiveTickerStatus }) {
  const ind = t.indicators;
  if (!ind) {
    const msg = t.warmupStatus ?? "Warming up…";
    return (
      <tr className="indicator-row">
        <td colSpan={10} className="indicator-cell">
          <span className="muted" style={{ fontSize: 11 }}>⏳ {msg}</span>
        </td>
      </tr>
    );
  }

  return (
    <tr className="indicator-row">
      <td colSpan={10} className="indicator-cell">
        <span className="indicator-tag mono">
          TSI {ind.tsi >= 0 ? "+" : ""}{ind.tsi.toFixed(1)}
        </span>
        <span className="indicator-tag mono">
          ADX {ind.adx.toFixed(1)}
        </span>
        <span className="indicator-tag mono">
          ATR {ind.atr.toFixed(2)}
        </span>
        <span className="indicator-tag mono muted" title={`EMA fast=${ind.emaFast.toFixed(2)}, slow=${ind.emaSlow.toFixed(2)}`}>
          EMA {ind.emaFast > ind.emaSlow ? "F>S" : "F<S"}
        </span>
      </td>
    </tr>
  );
}
