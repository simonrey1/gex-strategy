import { useState, useEffect, useMemo, useCallback } from "react";
import type { MissedEntriesReport, MissedEntry } from "@shared/types";
import { ALL_TICKERS_SYMBOL } from "@shared/types";
import { MiniChart } from "./MiniChart";

function useMissedEntriesReport() {
  const [report, setReport] = useState<MissedEntriesReport | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/backtest/missed-entries")
      .then(r => r.ok ? r.json() : null)
      .then((data: MissedEntriesReport | null) => { setReport(data); setLoading(false); })
      .catch(() => setLoading(false));
  }, []);

  return { report, loading };
}

type FilterMode = "all" | "sole" | string;

function GateTag({ gate, sole }: { gate: string; sole?: boolean }) {
  return (
    <span
      className="mono"
      style={{
        display: "inline-block",
        padding: "1px 6px",
        borderRadius: 3,
        fontSize: 11,
        background: sole ? "#f57c00" : "var(--bg-card)",
        color: sole ? "#fff" : "var(--text-muted)",
        border: sole ? "none" : "1px solid var(--border)",
        marginRight: 4,
      }}
    >
      {gate}
    </span>
  );
}

function EntryCard({ entry }: { entry: MissedEntry }) {
  return (
    <div style={{
      background: "var(--bg-card)",
      border: "1px solid var(--border)",
      borderRadius: 6,
      position: "relative",
    }}>
      <div style={{ padding: "8px 12px", borderBottom: "1px solid var(--border)" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
          <span className="mono" style={{ fontWeight: 600, fontSize: 13 }}>
            {entry.ticker}
          </span>
          <span
            className={`mono ${entry.profitPct >= 0 ? "positive" : "negative"}`}
            style={{ fontSize: 13, fontWeight: 600 }}
          >
            {entry.profitPct >= 0 ? "+" : ""}{entry.profitPct.toFixed(1)}%
          </span>
        </div>
        <div className="mono muted" style={{ fontSize: 11, marginBottom: 4 }}>
          {entry.entryTime.slice(0, 16)}
        </div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 2, marginBottom: 4 }}>
          {entry.failedGates.map(g => (
            <GateTag key={g} gate={g} sole={entry.soleGate === g} />
          ))}
        </div>
        <div className="mono muted" style={{ fontSize: 10, lineHeight: 1.6 }}>
          gpos={entry.entrySnapshot.gammaPos.toFixed(2)}
          {" "}atr={entry.entrySnapshot.atrPct.toFixed(3)}
          {" "}tsi={entry.entrySnapshot.tsi.toFixed(1)}
          {" "}adx={entry.entrySnapshot.adx.toFixed(1)}
          {entry.entrySnapshot.cwVsScwAtr != null && <>{" "}cw_scw={entry.entrySnapshot.cwVsScwAtr.toFixed(2)}</>}
          {entry.entrySnapshot.pwVsSpwAtr != null && <>{" "}pw_spw={entry.entrySnapshot.pwVsSpwAtr.toFixed(2)}</>}
          {entry.entrySnapshot.wallSpreadAtr != null && <>{" "}spread={entry.entrySnapshot.wallSpreadAtr.toFixed(1)}</>}
          {" "}iv_c={entry.entrySnapshot.ivCompressionRatio.toFixed(3)}
        </div>
      </div>
      <MiniChart
        bars={entry.bars}
        smoothPutWall={entry.smoothPutWall}
        smoothCallWall={entry.smoothCallWall}
        entryTimeSec={entry.entryTimeSec}
        entryPrice={entry.entryPrice}
        spikeTooltips={entry.spikeTooltips}
        spikeStartSec={entry.spikeStartSec}
        height={180}
      />
    </div>
  );
}

export function MissedEntriesTab({ activeTicker }: { activeTicker: string | null }) {
  const { report, loading } = useMissedEntriesReport();
  const [filter, setFilter] = useState<FilterMode>("all");

  const isAll = !activeTicker || activeTicker === ALL_TICKERS_SYMBOL;

  const tickerEntries = useMemo(() => {
    if (!report) return [];
    if (isAll) return report.entries;
    return report.entries.filter(e => e.ticker === activeTicker);
  }, [report, isAll, activeTicker]);

  const gates = useMemo(() => {
    const gateSet = new Set(tickerEntries.flatMap(e => e.failedGates));
    return [...gateSet];
  }, [tickerEntries]);

  const filtered = useMemo(() => {
    if (filter === "all") return tickerEntries;
    if (filter === "sole") return tickerEntries.filter(e => e.soleGate != null);
    return tickerEntries.filter(e => e.failedGates.includes(filter));
  }, [tickerEntries, filter]);

  const handleFilter = useCallback((f: FilterMode) => setFilter(f), []);

  if (loading) return <div className="page"><p className="muted">Loading missed entries...</p></div>;
  if (!report) return (
    <div className="page">
      <p className="muted">
        No missed entries data. Run with <code className="mono">--missed-entries -p 8080</code> to generate.
      </p>
    </div>
  );

  return (
    <div style={{ padding: "16px 28px", overflow: "auto", flex: 1 }}>
      <div style={{ marginBottom: 16 }}>
        <p className="muted" style={{ marginBottom: 8, fontSize: 12 }}>
          IV scan simulates ideal entries at every compression bar. These are the best ones ({">"}3% profit, low drawdown)
          that the VF entry gates rejected. Each card shows why.
        </p>
        <div className="mono muted" style={{ marginBottom: 8, fontSize: 12 }}>
          {tickerEntries.length} missed entries{isAll ? ` (${report.totalBest} best total, avg +${report.avgBestProfit.toFixed(1)}%)` : ""}
        </div>

        {isAll && (
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 12 }}>
            {report.summaries.map(s => (
              <div
                key={s.gate}
                style={{
                  background: "var(--bg-card)",
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                  padding: "6px 10px",
                  fontSize: 12,
                }}
              >
                <span className="mono" style={{ fontWeight: 600 }}>{s.gate}</span>
                <span className="muted"> {s.count}/{report.totalBest}</span>
                <span className="negative"> +{s.profitSum.toFixed(0)}%</span>
                {s.soleCount > 0 && (
                  <span style={{ color: "#f57c00" }}> sole={s.soleCount} +{s.soleProfitSum.toFixed(0)}%</span>
                )}
              </div>
            ))}
          </div>
        )}

        <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
          <button
            className={`tab ${filter === "all" ? "active" : ""}`}
            onClick={() => handleFilter("all")}
            style={{ fontSize: 11, padding: "4px 10px" }}
          >
            All ({tickerEntries.length})
          </button>
          <button
            className={`tab ${filter === "sole" ? "active" : ""}`}
            onClick={() => handleFilter("sole")}
            style={{ fontSize: 11, padding: "4px 10px" }}
          >
            Sole-blocked
          </button>
          {gates.map(g => (
            <button
              key={g}
              className={`tab ${filter === g ? "active" : ""}`}
              onClick={() => handleFilter(g)}
              style={{ fontSize: 11, padding: "4px 10px" }}
            >
              {g}
            </button>
          ))}
        </div>
      </div>

      <div style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))",
        gap: 12,
      }}>
        {filtered.map(entry => (
          <EntryCard key={`${entry.ticker}-${entry.entryTimeSec}`} entry={entry} />
        ))}
      </div>

      {filtered.length === 0 && (
        <p className="muted" style={{ marginTop: 20 }}>
          No entries match this filter.
        </p>
      )}
    </div>
  );
}
