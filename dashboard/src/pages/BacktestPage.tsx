import { useState, useCallback, useRef, useEffect, type DragEvent } from "react";
import type { IChartApi } from "lightweight-charts";
import { useBacktestResult } from "../hooks/useBacktestResult";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { StatsHeader } from "../components/StatsHeader";
import { TradesTable } from "../components/TradesTable";
import { DiagnosticsRow } from "../components/DiagnosticsRow";
import { SummaryCards } from "../components/SummaryCards";
import { PriceChart } from "../components/PriceChart";
import { ChartLegend } from "../components/ChartLegend";
import { Badge } from "../components/Badge";
import { TradeAnalysis } from "../components/TradeAnalysis";
import { MissedEntriesTab } from "../components/MissedEntriesTab";
import { etShort, fmtDuration, fmtSignedUsd, pnlClass } from "../lib/format";
import { ALL_TICKERS_SYMBOL, SIGNAL_BADGE, WALL_DEFS } from "../types";
import type { Column, BacktestTrade, Signal, WallKey } from "../types";

function makeColumns(showTicker: boolean): Column<BacktestTrade>[] {
  return [
    {
      key: "idx",
      header: "#",
      className: "mono",
      render: (t, i) => {
        const d = t.diagnostics;
        const hasWarn = d != null && d.callWallBelowEntry;
        return (
          <>
            {i + 1}
            {hasWarn && <span className="tag-warn"> !</span>}
          </>
        );
      },
    },
    ...(showTicker
      ? [{ key: "ticker", header: "Ticker", render: (t: BacktestTrade) => t.ticker, className: "mono" } as Column<BacktestTrade>]
      : []),
    {
      key: "type",
      header: "Type",
      render: (t) => {
        const b = SIGNAL_BADGE[t.signal as Exclude<Signal, "FLAT">];
        return b ? <Badge label={b.label} variant={b.variant} /> : null;
      },
    },
    { key: "entry", header: "Entry", render: (t) => etShort(t.entryTime), className: "mono" },
    { key: "exit", header: "Exit", render: (t) => etShort(t.exitTime), className: "mono" },
    {
      key: "dur",
      header: "Dur.",
      render: (t) => fmtDuration(t.entryTime, t.exitTime),
      className: "mono muted",
    },
    { key: "shares", header: "Shares", render: (t) => t.shares, className: "mono" },
    {
      key: "size",
      header: "Size",
      render: (t) => `$${((t.shares * t.entryPrice) / 1000).toFixed(1)}k`,
      className: "mono muted",
    },
    { key: "entryPx", header: "Entry $", render: (t) => `$${t.entryPrice.toFixed(2)}`, className: "mono" },
    { key: "exitPx", header: "Exit $", render: (t) => `$${t.exitPrice.toFixed(2)}`, className: "mono" },
    {
      key: "gross",
      header: "Gross",
      render: (t) => <span className={pnlClass(t.grossPnl)}>{fmtSignedUsd(t.grossPnl)}</span>,
      className: "mono",
    },
    {
      key: "comm",
      header: "Comm",
      render: (t) => <span className="negative">-${t.commission.toFixed(1)}</span>,
      className: "mono",
    },
    {
      key: "slip",
      header: "Slip",
      render: (t) => <span className="negative">-${t.slippage.toFixed(0)}</span>,
      className: "mono",
    },
    {
      key: "net",
      header: "Net P&L",
      render: (t) => {
        const grossWouldWin = t.grossPnl > 0 && t.netPnl <= 0;
        return (
          <span className={pnlClass(t.netPnl)}>
            {fmtSignedUsd(t.netPnl)}
            {grossWouldWin && <span className="tag-comm"> comm</span>}
          </span>
        );
      },
      className: "mono",
    },
    {
      key: "return",
      header: "Return",
      render: (t) => (
        <span className={pnlClass(t.returnPct)}>
          {t.returnPct >= 0 ? "+" : ""}
          {t.returnPct.toFixed(2)}%
        </span>
      ),
      className: "mono",
    },
    {
      key: "reason",
      header: "Exit Reason",
      render: (t) => {
        const reason = t.exitReason.replace("signal_flat ", "").replace(/[()]/g, "");
        return <span title={reason}>{reason}</span>;
      },
      className: "mono muted exit-reason",
    },
  ];
}

type Panel = "chart" | "trades" | "analysis" | "missed-entries";

const ALL_WALL_KEYS: WallKey[] = WALL_DEFS.map((d) => d.key);

function defaultHiddenWalls(): Set<WallKey> {
  const visible: WallKey[] = [
    "smoothPutWall", "smoothCallWall",
    "putWalls", "putWall2", "putWall3", "putWall4", "putWall5",
    "callWalls", "callWall2", "callWall3", "callWall4", "callWall5",
  ];
  return new Set(ALL_WALL_KEYS.filter((k) => !visible.includes(k)));
}

export function BacktestPage() {
  const {
    tickers, activeTicker, selectTicker,
    result, chartData,
    loading, error, loadFromFile, refresh,
  } = useBacktestResult();

  const [panel, setPanel] = useState<Panel>("chart");
  const [expandedRow, setExpandedRow] = useState<number | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [hiddenSeries, setHiddenSeries] = useState<Set<WallKey>>(() => defaultHiddenWalls());
  const [showSpikeWindows, setShowSpikeWindows] = useState(true);
  const priceChartRef = useRef<IChartApi | null>(null);
  const savedRangeRef = useRef<{ from: number; to: number } | null>(null);

  const handleRefresh = useCallback(() => {
    const ts = priceChartRef.current?.timeScale();
    const range = ts?.getVisibleRange();
    if (range) {
      savedRangeRef.current = { from: range.from as number, to: range.to as number };
    }
    void refresh();
  }, [refresh]);
  const isMultiTicker = activeTicker === ALL_TICKERS_SYMBOL;
  const columns = makeColumns(isMultiTicker);
  const hasPriceChart = chartData != null && chartData.bars.length > 0;
  const hasChart = !isMultiTicker;
  const activePanel = isMultiTicker && panel === "chart" ? "trades" : panel;

  const handleRowClick = useCallback(
    (index: number) => setExpandedRow((prev) => (prev === index ? null : index)),
    [],
  );

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      const file = e.dataTransfer.files[0];
      if (file) loadFromFile(file);
    },
    [loadFromFile],
  );

  const switchPanel = useCallback(
    (p: Panel) => {
      setPanel(p);
      if (p === "chart") {
        setTimeout(() => {
          priceChartRef.current?.timeScale().fitContent();
        }, 50);
      }
    },
    [],
  );

  useEffect(() => {
    setHiddenSeries(defaultHiddenWalls());
  }, [chartData]);

  useEffect(() => {
    if (!hasPriceChart) {
      priceChartRef.current = null;
    }
  }, [hasPriceChart]);

  if (loading && !result) {
    return <div className="page"><p className="muted">Loading backtest results...</p></div>;
  }

  if (!result) {
    return (
      <div
        className="page"
        onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
        onDragLeave={() => setDragOver(false)}
        onDrop={handleDrop}
        style={{
          border: dragOver ? "2px dashed var(--blue)" : "2px dashed transparent",
          borderRadius: 8,
          minHeight: 200,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div style={{ textAlign: "center" }}>
          <p className="muted">{error ?? "No backtest results available."}</p>
          <p className="muted" style={{ marginTop: 8, fontSize: 12 }}>
            Drop a state-*.json file here, or run a backtest first.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0, overflow: "hidden" }}>
      {tickers.length > 1 && (
        <div className="ticker-bar">
          {tickers.map((t) => (
            <button
              key={t}
              className={`ticker-btn ${t === activeTicker ? "active" : ""}`}
              onClick={() => selectTicker(t)}
            >
              {t}
            </button>
          ))}
        </div>
      )}

      <div style={{ padding: "20px 28px 12px" }}>
        <ErrorBoundary name="StatsHeader">
          <StatsHeader r={result} />
        </ErrorBoundary>
      </div>

      <div className="tabs" style={{ paddingLeft: 28 }}>
        {hasChart && (
          <button
            className={`tab ${activePanel === "chart" ? "active" : ""}`}
            onClick={() => switchPanel("chart")}
          >
            Chart
          </button>
        )}
        <button
          className={`tab ${activePanel === "trades" ? "active" : ""}`}
          onClick={() => switchPanel("trades")}
        >
          Trade List
        </button>
        <button
          className={`tab ${activePanel === "analysis" ? "active" : ""}`}
          onClick={() => switchPanel("analysis")}
        >
          Trade Analysis
        </button>
        <button
          className={`tab ${activePanel === "missed-entries" ? "active" : ""}`}
          onClick={() => switchPanel("missed-entries")}
        >
          Missed Entries
        </button>
      </div>

      {activePanel === "chart" && chartData && (
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {hasPriceChart && (
            <>
              <div style={{ display: "flex", alignItems: "center" }}>
                <ChartLegend
                  hidden={hiddenSeries}
                  onToggle={(key) => setHiddenSeries((prev) => {
                    const next = new Set(prev);
                    if (next.has(key)) next.delete(key); else next.add(key);
                    return next;
                  })}
                  onToggleGroup={(keys) => setHiddenSeries((prev) => {
                    const next = new Set(prev);
                    const allHidden = keys.every((k) => next.has(k));
                    for (const k of keys) { allHidden ? next.delete(k) : next.add(k); }
                    return next;
                  })}
                  showSpikeWindows={showSpikeWindows}
                  onToggleSpikeWindows={() => setShowSpikeWindows((v) => !v)}
                  hasSpikeWindows={(chartData.spikeWindows?.length ?? 0) > 0}
                />
                <button
                  onClick={handleRefresh}
                  disabled={loading}
                  style={{
                    marginLeft: "auto",
                    marginRight: 28,
                    padding: "3px 10px",
                    fontSize: 12,
                    background: "var(--surface)",
                    border: "1px solid var(--border)",
                    borderRadius: 4,
                    color: "var(--text-muted)",
                    cursor: loading ? "not-allowed" : "pointer",
                    display: "flex",
                    alignItems: "center",
                    gap: 4,
                  }}
                >
                  <span style={loading ? { display: "inline-block", animation: "spin 0.8s linear infinite" } : undefined}>↻</span>
                  Refresh
                </button>
              </div>
              <ErrorBoundary name="PriceChart">
                <PriceChart
                  data={chartData}
                  hiddenSeries={hiddenSeries}
                  showSpikeWindows={showSpikeWindows}
                  onChartReady={(c) => {
                    priceChartRef.current = c;
                    const sr = savedRangeRef.current;
                    if (sr) {
                      savedRangeRef.current = null;
                      // eslint-disable-next-line @typescript-eslint/no-explicit-any
                      c.timeScale().setVisibleRange(sr as any);
                    }
                  }}
                />
              </ErrorBoundary>
            </>
          )}
        </div>
      )}
      {activePanel === "chart" && !chartData && !loading && (
        <div className="page">
          <p className="muted">
            No chart data available. Re-run the backtest to generate chart data.
          </p>
        </div>
      )}

      {activePanel === "trades" && result && (
        <div style={{ flex: 1, overflow: "auto" }}>
          <div style={{ padding: "0 28px" }}>
            <ErrorBoundary name="SummaryCards">
              <SummaryCards r={result} />
            </ErrorBoundary>
          </div>
          <ErrorBoundary name="TradesTable">
            <TradesTable
              columns={columns}
              rows={result.trades}
              expandedRow={expandedRow}
              onRowClick={handleRowClick}
              renderExpanded={(trade) =>
                trade.diagnostics ? (
                  <ErrorBoundary name="DiagnosticsRow">
                    <DiagnosticsRow diagnostics={trade.diagnostics} />
                  </ErrorBoundary>
                ) : null
              }
              rowClassName={(t) => {
                const d = t.diagnostics;
                const warn = d != null && d.callWallBelowEntry;
                return `${t.netPnl > 0 ? "win" : "loss"}${warn ? " has-warning" : ""}`;
              }}
            />
          </ErrorBoundary>
        </div>
      )}

      {activePanel === "analysis" && result && (
        <ErrorBoundary name="TradeAnalysis">
          <TradeAnalysis result={result} activeTicker={activeTicker ?? ""} />
        </ErrorBoundary>
      )}

      {activePanel === "missed-entries" && (
        <ErrorBoundary name="MissedEntriesTab">
          <MissedEntriesTab activeTicker={activeTicker} />
        </ErrorBoundary>
      )}
    </div>
  );
}
