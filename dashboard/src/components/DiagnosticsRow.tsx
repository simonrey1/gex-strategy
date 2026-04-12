import type { TradeDiagnostics } from "../types";
import { fmtWall } from "../lib/format";

interface DiagnosticsRowProps {
  diagnostics: TradeDiagnostics;
  entryPutWall?: number | null;
  entryCallWall?: number | null;
}

export function DiagnosticsRow({ diagnostics: d }: DiagnosticsRowProps) {
  const warnings: string[] = [];
  if (d.callWallBelowEntry) warnings.push("Call wall <= entry price at exit");
  if (d.exitPutWall != null && d.entryPutWall != null && d.exitPutWall !== d.entryPutWall)
    warnings.push(`Put wall shifted ${fmtWall(d.entryPutWall)} -> ${fmtWall(d.exitPutWall)}`);
  if (d.exitCallWall != null && d.entryCallWall != null && d.exitCallWall !== d.entryCallWall)
    warnings.push(`Call wall shifted ${fmtWall(d.entryCallWall)} -> ${fmtWall(d.exitCallWall)}`);

  return (
    <div className="diag-grid">
      <div className="diag-section">
        <div className="diag-title">Entry Context</div>
        <DiagItem label="Reason" value={d.entryReason} />
        <DiagItem label="Zone Score" value={d.entryZoneScore.toFixed(2)} />
        <DiagItem label="Put Wall" value={fmtWall(d.entryPutWall)} />
        <DiagItem label="Call Wall" value={fmtWall(d.entryCallWall)} />
        <DiagItem label="Net GEX" value={d.entryNetGex.toFixed(0)} />
        <DiagItem label="ATR" value={`$${d.entryAtr.toFixed(3)}`} />
        <DiagItem label="ADX" value={d.entryAdx.toFixed(1)} />
      </div>
      <div className="diag-section">
        <div className="diag-title">Exit Context</div>
        <DiagItem label="Put Wall" value={fmtWall(d.exitPutWall)} />
        <DiagItem label="Call Wall" value={fmtWall(d.exitCallWall)} />
        <DiagItem label="Net GEX" value={d.exitNetGex.toFixed(0)} />
      </div>
      {warnings.length > 0 && (
        <div className="diag-section">
          <div className="diag-title">Warnings</div>
          {warnings.map((w) => (
            <div key={w} className="diag-warn">
              {w}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function DiagItem({ label, value, className }: { label: string; value: string; className?: string }) {
  return (
    <div className="diag-item">
      <span className="diag-label">{label}</span>
      <span className={className}>{value}</span>
    </div>
  );
}
