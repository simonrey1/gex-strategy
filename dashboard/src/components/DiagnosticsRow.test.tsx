import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { DiagnosticsRow } from "./DiagnosticsRow";
import type { TradeDiagnostics } from "../types";

const diag: TradeDiagnostics = {
  entryReason: "vanna_flip pw=$220 cw=$235 alpha=1.00 iv_peak=0.350 iv_now=0.200 bars=10 dist=2.1atr skew=0.50",
  entryPutWall: 220,
  entryCallWall: 235,
  entryNetGex: -50000,
  entryZoneScore: 3.0,
  entryAtr: 2.35,
  entryAdx: 22.1,
  signalBarTs: "2025-02-12T14:30:00Z",
  signalBarClose: 225.5,
  entryTsi: 15.0,
  entryPcGammaRatio: 0.5,
  entryAtmGammaDom: 0.0,
  entryNearGammaImbal: 0.0,
  entryPwComDistPct: 0.0,
  entryPwNearFarRatio: 0.0,
  entryPwDispersionAtr: 0.0,
  entryCwDispersionAtr: 0.0,
  exitPutWall: 220,
  exitCallWall: 235,
  exitNetGex: -48000,
  callWallBelowEntry: false,
};

describe("DiagnosticsRow", () => {
  it("renders entry context", () => {
    render(<DiagnosticsRow diagnostics={diag} />);
    expect(screen.getByText("Entry Context")).toBeInTheDocument();
    expect(screen.getByText(/vanna_flip/)).toBeInTheDocument();
    expect(screen.getAllByText("$220").length).toBeGreaterThanOrEqual(1);
  });

  it("renders exit context", () => {
    render(<DiagnosticsRow diagnostics={diag} />);
    expect(screen.getByText("Exit Context")).toBeInTheDocument();
    expect(screen.getByText("-48000")).toBeInTheDocument();
  });

  it("shows no warnings section when none apply", () => {
    render(<DiagnosticsRow diagnostics={diag} />);
    expect(screen.queryByText("Warnings")).not.toBeInTheDocument();
  });

  it("shows warnings when callWallBelowEntry is true", () => {
    render(<DiagnosticsRow diagnostics={{ ...diag, callWallBelowEntry: true }} />);
    expect(screen.getByText("Warnings")).toBeInTheDocument();
    expect(screen.getByText(/Call wall <= entry/)).toBeInTheDocument();
  });

  it("shows wall shift warning", () => {
    render(<DiagnosticsRow diagnostics={{ ...diag, exitPutWall: 218 }} />);
    expect(screen.getByText(/Put wall shifted/)).toBeInTheDocument();
  });
});
