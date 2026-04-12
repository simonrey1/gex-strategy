import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { MissedEntriesTab } from "./MissedEntriesTab";
import type { MissedEntriesReport } from "@shared/types";

const snap = {
  spot: 150, ivNow: 0.25, ivSpikeLevel: 0.40, ivCompressionRatio: 0.40,
  pwDistAtr: -1.0, cwDistAtr: 2.0, ivBaseRatio: 1.2, ivSpikeRatio: 1.8,
  atrAtSpike: 2.0, atrSpikeRatio: 0.9, cumIvDrop: -0.10, cumReturnAtr: 1.5,
  spikeMfeAtr: 2.0, spikeMaeAtr: -0.5, atrPct: 0.30, slowAtrPct: 0.35,
  cwVsScwAtr: -5.0, pwVsSpwAtr: -1.0, netGex: -1e8, gexAbsEma: 1e8,
  barsSinceSpike: 5, wallSpreadAtr: 4.0, gammaPos: 0.5, tsi: -20, adx: 20,
  atrRegimeRatio: 1.0,
  spikeVanna: 0.0, spikeGammaTilt: 0.0, pwDriftAtr: 0.0, netVanna: 0.0, gammaTilt: 0.0,
};

const mockReport: MissedEntriesReport = {
  totalBest: 2,
  avgBestProfit: 7.5,
  summaries: [
    { gate: "vf_cw_weak", count: 2, profitSum: 15, soleCount: 1, soleProfitSum: 8 },
  ],
  entries: [
    {
      ticker: "AAPL",
      entryTime: "2024-03-15T14:30:00Z",
      exitTime: "2024-03-20T15:00:00Z",
      entryPrice: 150, exitPrice: 162, atr: 2.0, profitPct: 8.0,
      maxRunupAtr: 3, exitReason: "tp", entryTimeSec: 1710510600,
      exitTimeSec: 1710943200, spikeBars: [100], bucket: "best",
      snapshot: snap, entrySnapshot: snap,
      failedGates: ["vf_cw_weak"], soleGate: "vf_cw_weak",
      bars: [], smoothPutWall: [], smoothCallWall: [], spikeTooltips: [], spikeStartSec: 0,
    },
    {
      ticker: "GOOG",
      entryTime: "2024-04-10T10:00:00Z",
      exitTime: "2024-04-15T15:00:00Z",
      entryPrice: 170, exitPrice: 181, atr: 3.0, profitPct: 7.0,
      maxRunupAtr: 2, exitReason: "tp", entryTimeSec: 1712743200,
      exitTimeSec: 1713193200, spikeBars: [200], bucket: "best",
      snapshot: snap, entrySnapshot: snap,
      failedGates: ["vf_cw_weak", "vf_spread_wide"], soleGate: null,
      bars: [], smoothPutWall: [], smoothCallWall: [], spikeTooltips: [], spikeStartSec: 0,
    },
  ],
};

function mockFetch(report: MissedEntriesReport | null) {
  vi.spyOn(globalThis, "fetch").mockImplementation(() => {
    if (report) return Promise.resolve(new Response(JSON.stringify(report)));
    return Promise.resolve(new Response("", { status: 404 }));
  });
}

beforeEach(() => vi.restoreAllMocks());

describe("MissedEntriesTab", () => {
  it("shows loading then entries", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker={null} />);
    expect(screen.getByText(/Loading/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("AAPL")).toBeInTheDocument());
    expect(screen.getByText("GOOG")).toBeInTheDocument();
  });

  it("shows no-data message when report missing", async () => {
    mockFetch(null);
    render(<MissedEntriesTab activeTicker={null} />);
    await waitFor(() =>
      expect(screen.getByText(/No missed entries data/)).toBeInTheDocument()
    );
  });

  it("filters by ticker when activeTicker set", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker="AAPL" />);
    await waitFor(() => expect(screen.getByText("AAPL")).toBeInTheDocument());
    expect(screen.queryByText("GOOG")).not.toBeInTheDocument();
  });

  it("shows all entries when ALL selected", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker="ALL" />);
    await waitFor(() => expect(screen.getByText("AAPL")).toBeInTheDocument());
    expect(screen.getByText("GOOG")).toBeInTheDocument();
  });

  it("displays gate tags on entry cards", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker={null} />);
    await waitFor(() => expect(screen.getAllByText("vf_cw_weak").length).toBeGreaterThanOrEqual(2));
  });

  it("can filter by sole-blocked", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker={null} />);
    await waitFor(() => expect(screen.getByText("AAPL")).toBeInTheDocument());
    fireEvent.click(screen.getByText("Sole-blocked"));
    expect(screen.getByText("AAPL")).toBeInTheDocument();
    expect(screen.queryByText("GOOG")).not.toBeInTheDocument();
  });

  it("displays profit percentage", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker={null} />);
    await waitFor(() => expect(screen.getByText("+8.0%")).toBeInTheDocument());
    expect(screen.getByText("+7.0%")).toBeInTheDocument();
  });

  it("displays gate summary cards in ALL view", async () => {
    mockFetch(mockReport);
    render(<MissedEntriesTab activeTicker="ALL" />);
    await waitFor(() => expect(screen.getByText(/2 missed entries/)).toBeInTheDocument());
    expect(screen.getByText(/2\/2/)).toBeInTheDocument();
  });
});
