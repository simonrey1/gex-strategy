import { useLiveTrades } from "../hooks/useLiveTrades";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { TradesTable } from "../components/TradesTable";
import { Badge } from "../components/Badge";
import {
  fmtUsd,
  fmtPct,
  pnlClass,
  ago,
  fmtUptime,
  pollHealthClass,
  fmtWall,
  etTime,
  etShort,
  etFull,
} from "../lib/format";
import { SIGNAL_BADGE, SIGNAL_FLAT } from "../types";
import type { Column, TradeRecord, LiveTickerStatus, LiveStatus, IbkrPosition, IbkrOrder, Signal } from "../types";

const columns: Column<TradeRecord>[] = [
  { key: "id", header: "#", render: (r) => r.id, className: "mono" },
  {
    key: "side",
    header: "Side",
    render: (r) => (
      <Badge label={r.side} variant={r.side === "ENTRY" ? "entry" : "exit"} />
    ),
  },
  {
    key: "time",
    header: "Time",
    render: (r) => etFull(r.timestamp),
    className: "mono",
  },
  { key: "ticker", header: "Ticker", render: (r) => <b>{r.ticker}</b> },
  { key: "signal", header: "Signal", render: (r) => {
    const b = SIGNAL_BADGE[r.signal as Exclude<Signal, "FLAT">];
    return b ? <Badge label={b.label} variant={b.variant} /> : null;
  }},
  { key: "shares", header: "Shares", render: (r) => r.shares, className: "mono" },
  { key: "price", header: "Price", render: (r) => fmtUsd(r.price), className: "mono" },
  { key: "sl", header: "SL", render: (r) => fmtUsd(r.stopLoss), className: "mono muted" },
  { key: "tp", header: "TP", render: (r) => fmtUsd(r.takeProfit), className: "mono muted" },
  {
    key: "pnl",
    header: "P&L",
    render: (r) => <span className={pnlClass(r.pnl)}>{fmtUsd(r.pnl)}</span>,
    className: "mono",
  },
  {
    key: "return",
    header: "Return",
    render: (r) => <span className={pnlClass(r.returnPct)}>{fmtPct(r.returnPct)}</span>,
    className: "mono",
  },
  {
    key: "reason",
    header: "Reason",
    render: (r) => <span title={r.reason ?? ""}>{r.reason}</span>,
    className: "mono muted exit-reason",
  },
];

function StatusDot({ status }: { status: "healthy" | "warn" | "stale" | "off" }) {
  return <span className={`status-dot dot-${status}`} />;
}

function fmtBarTime(iso: string | null): string {
  if (!iso) return "—";
  return etShort(iso);
}

function AccountHeader({ status }: { status: LiveStatus }) {
  const equity = status.tickers.find((t) => t.equity > 0)?.equity ?? 0;
  const gex = status.gexStream;
  const equityLoading = equity === 0;

  const ibkrOk = status.brokerConnected;
  const gexOk = gex?.phase === "live";
  const gexErr = gex?.phase === "error";

  return (
    <div className="live-header">
      <div className="live-header-row">
        <div className="header-group">
          <div className="header-item">
            <span className="header-label">Up</span>
            <span className="header-val mono">{fmtUptime(status.uptimeSeconds)}</span>
            <span className="muted mono" style={{ fontSize: 11 }}>
              since {etTime(status.upSince)}
            </span>
          </div>
          <div className="header-sep" />
          <div className="header-item" title={ibkrOk ? "IBKR connected" : "IBKR disconnected"}>
            <StatusDot status={ibkrOk ? "healthy" : "stale"} />
            <span style={{ fontSize: 11 }} className={ibkrOk ? "muted" : "negative"}>IBKR</span>
          </div>
          <div className="header-item" title={gexOk ? "GEX polling active" : gexErr ? `GEX error: ${gex?.lastError}` : "GEX starting…"}>
            <StatusDot status={gexOk ? "healthy" : gexErr ? "stale" : "warn"} />
            <span style={{ fontSize: 11 }} className={gexOk ? "muted" : gexErr ? "negative" : "warn"}>GEX</span>
          </div>
        </div>
        <div className="header-group">
          <div className="header-item">
            <span className="header-label">Equity</span>
            <span className="header-value-lg mono">
              {equityLoading ? <span className="muted">Loading…</span> : fmtUsd(equity)}
            </span>
          </div>
        </div>
      </div>
      {gex?.lastError && (
        <div className="gex-status-bar">
          <span className="negative gex-error">{gex.lastError}</span>
        </div>
      )}
    </div>
  );
}

function indicatorTooltip(t: LiveTickerStatus): string {
  const ind = t.indicators;
  if (!ind) return t.warmupStatus ?? "Warming up…";
  return `TSI ${ind.tsi >= 0 ? "+" : ""}${ind.tsi.toFixed(1)}  ADX ${ind.adx.toFixed(1)}  ATR ${ind.atr.toFixed(2)}  EMA ${ind.emaFast > ind.emaSlow ? "F>S" : "F<S"}`;
}

function TickerRow({ t }: { t: LiveTickerStatus }) {
  const ibkrHealth = pollHealthClass(t.lastPollMs);
  const gexHealth = pollHealthClass(t.lastGexMs ?? null);
  const sig = t.signal && t.signal !== SIGNAL_FLAT ? SIGNAL_BADGE[t.signal] : null;
  const posLabel = t.hasPosition ? (sig?.label ?? "OPEN") : SIGNAL_FLAT;

  return (
    <tr className="ticker-row" title={indicatorTooltip(t)}>
      <td className="ticker-name">{t.ticker}</td>
      <td className="mono">
        {t.spotPrice > 0 ? fmtUsd(t.spotPrice) : <span className="muted">—</span>}
      </td>
      <td className="mono muted">{fmtWall(t.putWall)}</td>
      <td className="mono muted">{fmtWall(t.callWall)}</td>
      <td>
        <span className={`pos-badge ${t.hasPosition ? "pos-open" : "pos-flat"}`}>
          {posLabel}
        </span>
      </td>
      <td className="mono muted">{fmtBarTime(t.lastBarTime)}</td>
      <td className="mono muted">{t.barsToday > 0 ? t.barsToday : "—"}</td>
      <td>
        <span className={`poll-indicator poll-${ibkrHealth}`}>
          <StatusDot status={ibkrHealth} />
          <span>{ago(t.lastPollMs)}</span>
        </span>
      </td>
      <td>
        <span className={`poll-indicator poll-${gexHealth}`}>
          <StatusDot status={gexHealth} />
          <span>{ago(t.lastGexMs ?? null)}</span>
        </span>
      </td>
      <td className="ticker-error-cell">
        {t.consecutiveFailures > 0 ? (
          <span className="negative mono">{t.consecutiveFailures} fail{t.consecutiveFailures > 1 ? "s" : ""}</span>
        ) : t.lastError ? (
          <span className="negative">{t.lastError}</span>
        ) : (
          <span className="muted">OK</span>
        )}
      </td>
    </tr>
  );
}

function TickerTable({ tickers }: { tickers: LiveTickerStatus[] }) {
  return (
    <div className="ticker-table-wrap">
      <table className="ticker-table">
        <thead>
          <tr>
            <th>Ticker</th>
            <th>Spot</th>
            <th>Put Wall</th>
            <th>Call Wall</th>
            <th>Position</th>
            <th>Last Bar</th>
            <th>Bars</th>
            <th>IBKR</th>
            <th>GEX</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {tickers.map((t) => (
            <TickerRow key={t.ticker} t={t} />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PositionsTable({ positions }: { positions: IbkrPosition[] | null }) {
  if (!positions) return <div className="muted" style={{ padding: "12px 0" }}>Loading…</div>;
  if (positions.length === 0) {
    return <div className="muted" style={{ padding: "12px 0" }}>No open positions</div>;
  }
  return (
    <table className="ticker-table">
      <thead>
        <tr>
          <th>Symbol</th>
          <th>Shares</th>
          <th>Avg Cost</th>
          <th>Value</th>
        </tr>
      </thead>
      <tbody>
        {positions.map((p) => (
          <tr key={p.symbol} className="ticker-row">
            <td className="ticker-name">{p.symbol}</td>
            <td className="mono">{p.shares}</td>
            <td className="mono">{fmtUsd(p.avgCost)}</td>
            <td className="mono">{fmtUsd(p.shares * p.avgCost)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function OrdersTable({ orders }: { orders: IbkrOrder[] | null }) {
  if (!orders) return <div className="muted" style={{ padding: "12px 0" }}>Loading…</div>;
  if (orders.length === 0) {
    return <div className="muted" style={{ padding: "12px 0" }}>No open orders</div>;
  }
  return (
    <table className="ticker-table">
      <thead>
        <tr>
          <th>ID</th>
          <th>Symbol</th>
          <th>Side</th>
          <th>Type</th>
          <th>Qty</th>
          <th>Limit</th>
          <th>Stop</th>
          <th>Status</th>
          <th>Filled</th>
          <th>Remaining</th>
        </tr>
      </thead>
      <tbody>
        {orders.map((o) => (
          <tr key={o.orderId} className="ticker-row">
            <td className="mono">{o.orderId}</td>
            <td className="ticker-name">{o.symbol}</td>
            <td>{o.action}</td>
            <td className="mono">{o.orderType}</td>
            <td className="mono">{o.quantity}</td>
            <td className="mono">{o.limitPrice != null ? fmtUsd(o.limitPrice) : "—"}</td>
            <td className="mono">{o.stopPrice != null ? fmtUsd(o.stopPrice) : "—"}</td>
            <td className="mono">{o.status}</td>
            <td className="mono">{o.filled}</td>
            <td className="mono">{o.remaining}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function EmptyTradeLog() {
  return (
    <div className="empty-trades">
      <div className="empty-trades-pulse" />
      <div className="empty-trades-text">
        <span>Waiting for signals…</span>
        <span className="muted" style={{ fontSize: 11 }}>
          Trades will appear here when the strategy opens a position
        </span>
      </div>
    </div>
  );
}

export function LivePage() {
  const { trades, status, positions, orders, error } = useLiveTrades();
  const sorted = [...trades].reverse();

  return (
    <div className="page">
      {error && (
        <div className="live-error">
          <StatusDot status="stale" />
          {error}
        </div>
      )}

      {status && (
        <ErrorBoundary name="AccountHeader">
          <AccountHeader status={status} />
        </ErrorBoundary>
      )}

      {status && (
        <div className="section">
          <div className="section-title">Tickers</div>
          <ErrorBoundary name="TickerTable">
            <TickerTable tickers={status.tickers} />
          </ErrorBoundary>
        </div>
      )}

      <div className="section">
        <div className="section-title">IBKR Positions</div>
        <ErrorBoundary name="PositionsTable">
          <PositionsTable positions={positions} />
        </ErrorBoundary>
      </div>

      <div className="section">
        <div className="section-title">IBKR Orders</div>
        <ErrorBoundary name="OrdersTable">
          <OrdersTable orders={orders} />
        </ErrorBoundary>
      </div>

      <div className="section">
        <div className="section-title">Trade Log</div>
        <ErrorBoundary name="TradeLog">
          {sorted.length > 0 ? (
            <TradesTable columns={columns} rows={sorted} />
          ) : (
            <EmptyTradeLog />
          )}
        </ErrorBoundary>
      </div>
    </div>
  );
}
