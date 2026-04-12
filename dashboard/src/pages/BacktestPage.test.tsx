import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { BacktestPage } from "./BacktestPage";
import type { SavedBacktestState, BacktestTrade } from "../types";

const trade: BacktestTrade = {
  ticker: "AAPL",
  signal: "LONG_VANNA_FLIP",
  entryTime: "2024-11-15T14:30:00Z",
  entryPrice: 225,
  exitTime: "2024-11-15T15:15:00Z",
  exitPrice: 227.5,
  shares: 44,
  grossPnl: 110,
  commission: 0.88,
  slippage: 4.4,
  netPnl: 104.72,
  returnPct: 1.06,
  exitReason: "take_profit",
  maxRunupAtr: 3.5,
  barsHeld: 10,
  spikeBar: 0,
  diagnostics: null,
};

const mockState: SavedBacktestState = {
  version: 1,
  savedAt: "2025-01-01T00:00:00Z",
  chartData: null,
  result: {
    ticker: "AAPL",
    label: "AAPL",
    startDate: "2024-11-01",
    endDate: "2025-01-31",
    totalBars: 5000,
    totalTrades: 1,
    winners: 1,
    losers: 0,
    winRate: 1,
    grossPnl: 110,
    totalCommission: 0.88,
    totalSlippage: 4.4,
    netPnl: 104.72,
    totalReturnPct: 1.06,
    profitFactor: 999,
    maxDrawdown: 0,
    maxDrawdownPct: 0,
    sharpeRatio: 2.5,
    sortinoRatio: 3.0,
    calmarRatio: 1.0,
    cagr: 0.01,
    expectancy: 104.72,
    payoffRatio: null,
    ulcerIndex: 0.0,
    maxDdDurationDays: 0,
    avgTradeDurationMinutes: 45,
    avgWinPct: 1.06,
    avgLossPct: 0,
    avgTradePct: 1.06,
    buyHoldReturnPct: 0.5,
    alphaPct: 0.56,
    monthlyReturns: [],
    trades: [trade],
    tradeAnalysis: { highRunupLosers: [], worstLosses: [] },
    wallEvents: [],
    avgCapitalUtilPct: 0.5,
  },
};

function mockFetch() {
  vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
    const u = typeof url === "string" ? url : url.toString();
    if (u.includes("/api/backtest/tickers"))
      return Promise.resolve(new Response(JSON.stringify(["AAPL"])));
    if (u.includes("/api/backtest"))
      return Promise.resolve(new Response(JSON.stringify(mockState)));
    return Promise.reject(new Error("unexpected"));
  });
}

beforeEach(() => {
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

describe("BacktestPage", () => {
  it("renders stats header and chart tab by default", async () => {
    mockFetch();
    render(<BacktestPage />);

    await waitFor(() => {
      expect(screen.getByText(/AAPL/)).toBeInTheDocument();
    });

    expect(screen.getByText("Chart")).toBeInTheDocument();
    expect(screen.getByText("Trade List")).toBeInTheDocument();
  });

  it("switches to trade list and shows trades table", async () => {
    mockFetch();
    render(<BacktestPage />);

    await waitFor(() => {
      expect(screen.getByText(/AAPL/)).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("Trade List"));

    expect(screen.getByText("VF")).toBeInTheDocument();
    expect(screen.getByText("take_profit")).toBeInTheDocument();
  });

  it("shows loading state", () => {
    vi.spyOn(globalThis, "fetch").mockReturnValue(new Promise(() => {}));
    render(<BacktestPage />);
    expect(screen.getByText(/Loading/)).toBeInTheDocument();
  });

  it("shows drop zone when no results", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      if (u.includes("/tickers")) return Promise.resolve(new Response(JSON.stringify([])));
      return Promise.resolve(new Response("not found", { status: 404 }));
    });

    render(<BacktestPage />);

    await waitFor(() => {
      expect(screen.getByText("No backtest results available.")).toBeInTheDocument();
    });
  });
});
