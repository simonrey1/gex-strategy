import type { BacktestResult } from "../types";

interface StatsHeaderProps {
  r: BacktestResult;
}

function fmtSign(v: number, decimals = 2): string {
  return `${v >= 0 ? "+" : ""}${v.toFixed(decimals)}`;
}

export function StatsHeader({ r }: StatsHeaderProps) {
  const commPct = r.grossPnl !== 0 ? Math.abs((r.totalCommission / r.grossPnl) * 100) : 0;

  return (
    <div className="page-header">
      <h2>
        {r.label || r.ticker} &mdash; {r.startDate} &rarr; {r.endDate}
      </h2>

      {/* ── Strategy vs Buy & Hold (inline) ── */}
      <div className="vs-row" style={{ marginTop: 14 }}>
        <span className="vs-label">Strategy</span>
        <span className={`vs-num ${r.totalReturnPct >= 0 ? "positive" : "negative"}`}>
          {fmtSign(r.totalReturnPct)}%
        </span>
        <span className="vs-sep">vs</span>
        <span className="vs-label">B&amp;H</span>
        <span className={`vs-num ${r.buyHoldReturnPct >= 0 ? "positive" : "negative"}`}>
          {fmtSign(r.buyHoldReturnPct)}%
        </span>
        <span className="vs-eq">=</span>
        <span className="vs-label">Alpha</span>
        <span className={`vs-num vs-alpha ${r.alphaPct >= 0 ? "positive" : "negative"}`}>
          {fmtSign(r.alphaPct)}%
        </span>
        <span className="vs-net muted">({r.netPnl >= 0 ? "+" : ""}${r.netPnl.toFixed(0)} net)</span>
      </div>

      <div className="stats-grid" style={{ marginTop: 14 }}>
        <Stat label="Trades" value={String(r.totalTrades)} sub={`${r.winners}W / ${r.losers}L (${(r.winRate * 100).toFixed(0)}%)`} />
        <Stat label="Profit Factor" value={r.profitFactor == null || !isFinite(r.profitFactor) ? "∞" : r.profitFactor.toFixed(2)} />
        <Stat label="Sharpe" value={r.sharpeRatio.toFixed(2)} sub={r.totalTrades < 30 ? "low sample" : undefined} subCls={r.totalTrades < 30 ? "warn" : undefined} />
        <Stat label="Max Drawdown" value={`${(r.maxDrawdownPct * 100).toFixed(2)}%`} cls="negative" sub={`$${r.maxDrawdown.toFixed(0)}`} />
        <Stat label="Commission" value={`-$${r.totalCommission.toFixed(0)}`} cls="negative" sub={`${commPct.toFixed(0)}% of gross`} />
        <Stat label="Slippage" value={`-$${r.totalSlippage.toFixed(0)}`} cls="warn" />
        <Stat
          label="Avg Win / Loss"
          value=""
          custom={
            <>
              <span className="positive">+{r.avgWinPct.toFixed(2)}%</span>
              <span className="muted"> / </span>
              <span className="negative">{r.avgLossPct.toFixed(2)}%</span>
            </>
          }
        />
      </div>
      {r.grossPnl > 0 && <PnlBar r={r} />}
    </div>
  );
}

function Stat({
  label,
  value,
  cls,
  sub,
  subCls,
  custom,
}: {
  label: string;
  value: string;
  cls?: string;
  sub?: string;
  subCls?: string;
  custom?: React.ReactNode;
}) {
  return (
    <div className="stat">
      <span className="stat-label">{label}</span>
      {custom ? (
        <span className="stat-value">{custom}</span>
      ) : (
        <span className={`stat-value ${cls ?? ""}`}>{value}</span>
      )}
      {sub && <span className={`stat-sub ${subCls ?? ""}`}>{sub}</span>}
    </div>
  );
}

function PnlBar({ r }: { r: BacktestResult }) {
  const netPct = Math.max(2, (r.netPnl / r.grossPnl) * 100);
  const commPct = Math.max(2, (r.totalCommission / r.grossPnl) * 100);
  const slipPct = Math.max(1, (r.totalSlippage / r.grossPnl) * 100);
  return (
    <div className="pnl-breakdown">
      <div className="pnl-bar">
        <div className="pnl-bar-gross" style={{ width: `${netPct.toFixed(1)}%` }} />
        <div className="pnl-bar-comm" style={{ width: `${commPct.toFixed(1)}%` }} />
        <div className="pnl-bar-slip" style={{ width: `${slipPct.toFixed(1)}%` }} />
      </div>
      <div className="pnl-labels">
        <div className="pnl-label">
          <div className="pnl-dot" style={{ background: "var(--positive)" }} />
          <span className="muted">Net</span>
        </div>
        <div className="pnl-label">
          <div className="pnl-dot" style={{ background: "var(--negative)" }} />
          <span className="muted">Comm</span>
        </div>
        <div className="pnl-label">
          <div className="pnl-dot" style={{ background: "var(--warn)" }} />
          <span className="muted">Slip</span>
        </div>
      </div>
    </div>
  );
}
