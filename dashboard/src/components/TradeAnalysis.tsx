import { useState } from "react";
import { etShort, fmtSignedUsd, pnlClass, fmtDuration } from "../lib/format";
import { SIGNAL_BADGE, ALL_TICKERS_SYMBOL } from "../types";
import { Badge } from "./Badge";
import type { BacktestTrade, BacktestResult, Signal } from "../types";

type Section = "runup" | "worst";

function AnalysisTable({ trades, showTicker }: { trades: BacktestTrade[]; showTicker: boolean }) {
  if (trades.length === 0) return <p className="muted" style={{ padding: "12px 28px" }}>No trades match.</p>;
  return (
    <div className="trade-table-wrap">
      <table className="trade-table">
        <thead>
          <tr>
            <th>#</th>
            {showTicker && <th>Ticker</th>}
            <th>Type</th>
            <th>Entry</th>
            <th>Exit</th>
            <th>Dur.</th>
            <th>Entry $</th>
            <th>Exit $</th>
            <th>Net P&L</th>
            <th>Return</th>
            <th>Runup</th>
            <th>Bars</th>
            <th>Exit Reason</th>
          </tr>
        </thead>
        <tbody>
          {trades.map((t, i) => {
            const b = SIGNAL_BADGE[t.signal as Exclude<Signal, "FLAT">];
            const reason = t.exitReason.replace("signal_flat ", "").replace(/[()]/g, "");
            return (
              <tr key={i} className={t.netPnl > 0 ? "win" : "loss"}>
                <td className="mono">{i + 1}</td>
                {showTicker && <td className="mono">{t.ticker}</td>}
                <td>{b ? <Badge label={b.label} variant={b.variant} /> : null}</td>
                <td className="mono">{etShort(t.entryTime)}</td>
                <td className="mono">{etShort(t.exitTime)}</td>
                <td className="mono muted">{fmtDuration(t.entryTime, t.exitTime)}</td>
                <td className="mono">${t.entryPrice.toFixed(2)}</td>
                <td className="mono">${t.exitPrice.toFixed(2)}</td>
                <td className="mono"><span className={pnlClass(t.netPnl)}>{fmtSignedUsd(t.netPnl)}</span></td>
                <td className="mono">
                  <span className={pnlClass(t.returnPct)}>
                    {t.returnPct >= 0 ? "+" : ""}{t.returnPct.toFixed(2)}%
                  </span>
                </td>
                <td className="mono">{t.maxRunupAtr.toFixed(1)} ATR</td>
                <td className="mono muted">{t.barsHeld}</td>
                <td className="mono muted exit-reason" title={reason}>{reason}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function TradeAnalysis({ result, activeTicker }: { result: BacktestResult; activeTicker: string }) {
  const [section, setSection] = useState<Section>("runup");
  const showTicker = activeTicker === ALL_TICKERS_SYMBOL;
  const { trades, tradeAnalysis } = result;

  const highRunupLosers = tradeAnalysis.highRunupLosers.flatMap((i) => trades[i] ? [trades[i]] : []);
  const worstLosses = tradeAnalysis.worstLosses.flatMap((i) => trades[i] ? [trades[i]] : []);

  return (
    <div>
      <div className="tabs" style={{ paddingLeft: 28, marginBottom: 0, borderBottom: "none" }}>
        <button className={`tab ${section === "runup" ? "active" : ""}`} onClick={() => setSection("runup")}>
          High Runup Losers ({highRunupLosers.length})
        </button>
        <button className={`tab ${section === "worst" ? "active" : ""}`} onClick={() => setSection("worst")}>
          Worst Losses ({worstLosses.length})
        </button>
      </div>

      {section === "runup" && (
        <>
          <p className="muted" style={{ padding: "4px 28px 8px", fontSize: 12 }}>
            Trades that ran up ≥2 ATR but ended as losses — reveals TP/trailing stop problems.
          </p>
          <AnalysisTable trades={highRunupLosers} showTicker={showTicker} />
        </>
      )}

      {section === "worst" && (
        <>
          <p className="muted" style={{ padding: "4px 28px 8px", fontSize: 12 }}>
            All losing trades sorted by dollar P&L (worst first).
          </p>
          <AnalysisTable trades={worstLosses} showTicker={showTicker} />
        </>
      )}
    </div>
  );
}
