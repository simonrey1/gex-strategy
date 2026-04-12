import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useBacktestResult } from "./useBacktestResult";
import type { SavedBacktestState, BacktestResult } from "../types";

const mockResult: BacktestResult = {
  ticker: "AAPL",
  label: "AAPL",
  startDate: "2024-11-01",
  endDate: "2025-01-31",
  totalBars: 5000,
  totalTrades: 10,
  winners: 6,
  losers: 4,
  winRate: 0.6,
  grossPnl: 500,
  totalCommission: 20,
  totalSlippage: 10,
  netPnl: 470,
  totalReturnPct: 4.7,
  profitFactor: 1.8,
  maxDrawdown: 200,
  maxDrawdownPct: 0.02,
  sharpeRatio: 1.2,
  sortinoRatio: 1.8,
  calmarRatio: 0.4,
  cagr: 0.05,
  expectancy: 47.0,
  payoffRatio: 1.67,
  ulcerIndex: 0.01,
  maxDdDurationDays: 15,
  avgTradeDurationMinutes: 90,
  avgWinPct: 0.5,
  avgLossPct: -0.3,
  avgTradePct: 0.15,
  buyHoldReturnPct: 3.0,
  alphaPct: 1.7,
  monthlyReturns: [],
  trades: [],
  tradeAnalysis: { highRunupLosers: [], worstLosses: [] },
  wallEvents: [],
  avgCapitalUtilPct: 0.5,
};

const mockState: SavedBacktestState = {
  version: 1,
  savedAt: "2025-01-01T00:00:00Z",
  result: mockResult,
  chartData: null,
};

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("useBacktestResult", () => {
  it("loads tickers then fetches first ticker result", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      if (u.includes("/api/backtest/tickers")) {
        return Promise.resolve(new Response(JSON.stringify(["AAPL", "GOOG"])));
      }
      if (u.includes("/api/backtest")) {
        return Promise.resolve(new Response(JSON.stringify(mockState)));
      }
      return Promise.reject(new Error("unexpected"));
    });

    const { result } = renderHook(() => useBacktestResult());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.tickers).toEqual(["AAPL", "GOOG"]);
    expect(result.current.activeTicker).toBe("AAPL");
    expect(result.current.result).not.toBeNull();
    expect(result.current.result!.ticker).toBe("AAPL");
    expect(result.current.chartData).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it("sets error on non-200 response", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      if (u.includes("/tickers")) return Promise.resolve(new Response(JSON.stringify(["X"])));
      return Promise.resolve(new Response("not found", { status: 404 }));
    });

    const { result } = renderHook(() => useBacktestResult());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toContain("404");
    expect(result.current.result).toBeNull();
  });

  it("sets error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      if (u.includes("/tickers")) return Promise.resolve(new Response(JSON.stringify(["X"])));
      return Promise.reject(new Error("offline"));
    });

    const { result } = renderHook(() => useBacktestResult());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe("offline");
  });
});
