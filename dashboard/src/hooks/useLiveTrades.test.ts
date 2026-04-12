import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useLiveTrades } from "./useLiveTrades";
import type { LiveStatus, TradeRecord } from "../types";

const mockStatus: LiveStatus = {
  status: "ok",
  brokerConnected: true,
  upSince: "2025-01-01T00:00:00Z",
  uptimeSeconds: 3600,
  tickers: [
    {
      ticker: "AAPL",
      lastPollMs: Date.now(),
      lastPollAgoSeconds: 5,
      hasPosition: false,
      signal: null,
      spotPrice: 225.5,
      lastBarTime: "2025-01-01T15:30:00Z",
      barsToday: 330,
      equity: 100000,
      consecutiveFailures: 0,
      lastError: null,
      putWall: null,
      callWall: null,
      netGex: null,
      lastGexMs: null,
      lastGexAgoSeconds: null,
      indicators: null,
      warmupStatus: null,
    },
  ],
  gexStream: null,
};

const mockTrades: TradeRecord[] = [
  {
    id: 1,
    ticker: "AAPL",
    signal: "LONG_VANNA_FLIP",
    side: "ENTRY",
    reason: "bounce",
    shares: 10,
    price: 225.5,
    stopLoss: 220,
    takeProfit: 235,
    pnl: null,
    returnPct: null,
    equity: 100000,
    timestamp: "2025-01-01T10:30:00Z",
  },
];

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("useLiveTrades", () => {
  it("fetches status and trades on mount", async () => {
    const jsonRes = (body: unknown) =>
      new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });

    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      if (u.includes("/api/status")) return Promise.resolve(jsonRes(mockStatus));
      if (u.includes("/api/trades")) return Promise.resolve(jsonRes(mockTrades));
      if (u.includes("/api/positions")) return Promise.resolve(jsonRes([]));
      if (u.includes("/api/orders")) return Promise.resolve(jsonRes([]));
      return Promise.reject(new Error("unexpected"));
    });

    const { result } = renderHook(() => useLiveTrades());

    await waitFor(() => {
      expect(result.current.status).not.toBeNull();
    });

    expect(result.current.status!.tickers[0]!.ticker).toBe("AAPL");
    expect(result.current.trades).toHaveLength(1);
    expect(result.current.positions).toHaveLength(0);
    expect(result.current.orders).toHaveLength(0);
    expect(result.current.error).toBeNull();
  });

  it("sets error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("network down"));

    const { result } = renderHook(() => useLiveTrades());

    await waitFor(() => {
      expect(result.current.error).toBe("Live server not connected");
    });
  });
});
