import type { BacktestResult } from "@shared/types";
import { pnlClass } from "../lib/format";

interface SummaryCardsProps {
  r: BacktestResult;
}

export function SummaryCards({ r }: SummaryCardsProps) {
  const pnlSign = r.netPnl >= 0 ? "+" : "";
  const grossSign = r.grossPnl >= 0 ? "+" : "";
  const costsPct = r.grossPnl > 0 && r.netPnl < r.grossPnl
    ? `${((r.totalCommission + r.totalSlippage) / r.grossPnl * 100).toFixed(0)}% of gross profit eaten by costs`
    : "Costs breakdown";

  return (
    <div className="cards" style={{ padding: "16px 0" }}>
      <div className="card">
        <div className="card-label">Net P&L</div>
        <div className={`card-value ${pnlClass(r.netPnl)}`}>
          {pnlSign}${r.netPnl.toFixed(2)}
        </div>
        <div className="card-detail">
          Gross {grossSign}${r.grossPnl.toFixed(2)} &minus; ${r.totalCommission.toFixed(2)} comm &minus; ${r.totalSlippage.toFixed(2)} slip
        </div>
      </div>
      <div className="card">
        <div className="card-label">Costs Impact</div>
        <div className={`card-value ${r.totalCommission > Math.abs(r.grossPnl) ? "negative" : "warn"}`}>
          -${(r.totalCommission + r.totalSlippage).toFixed(0)}
        </div>
        <div className="card-detail">{costsPct}</div>
      </div>
      <div className="card">
        <div className="card-label">Per Trade Avg</div>
        <div className={`card-value ${pnlClass(r.avgTradePct)}`}>
          {r.avgTradePct >= 0 ? "+" : ""}{r.avgTradePct.toFixed(2)}%
        </div>
        <div className="card-detail">
          ~${r.totalTrades > 0 ? (r.totalCommission / r.totalTrades).toFixed(0) : 0} comm/trade
          &middot; ~${r.totalTrades > 0 ? (r.totalSlippage / r.totalTrades).toFixed(0) : 0} slip/trade
        </div>
      </div>
    </div>
  );
}
