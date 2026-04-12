import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { LivePage } from "./LivePage";
import type { LiveStatus, TradeRecord, IbkrPosition, IbkrOrder } from "../types";

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
      hasPosition: true,
      signal: "LONG_VANNA_FLIP",
      spotPrice: 225.5,
      lastBarTime: "2025-01-01T15:30:00Z",
      barsToday: 330,
      equity: 100000,
      consecutiveFailures: 0,
      lastError: null,
      putWall: 220.0,
      callWall: 230.0,
      netGex: null,
      lastGexMs: Date.now(),
      lastGexAgoSeconds: 10,
      indicators: {
        atr: 2.5,
        emaFast: 226.0,
        emaSlow: 224.0,
        adx: 22.5,
        tsi: 15.3,
        tsiBullish: true,
      },
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

const mockPositions: IbkrPosition[] = [
  { symbol: "AAPL", shares: 10, avgCost: 225.5 },
];

const mockOrders: IbkrOrder[] = [
  {
    orderId: 42,
    symbol: "AAPL",
    action: "Sell",
    orderType: "STP",
    quantity: 10,
    limitPrice: null,
    stopPrice: 220,
    status: "PreSubmitted",
    filled: 0,
    remaining: 10,
  },
];

beforeEach(() => {
  vi.restoreAllMocks();
  const jsonRes = (body: unknown) =>
    new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });

  vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
    const u = typeof url === "string" ? url : url.toString();
    if (u.includes("/api/status")) return Promise.resolve(jsonRes(mockStatus));
    if (u.includes("/api/trades")) return Promise.resolve(jsonRes(mockTrades));
    if (u.includes("/api/positions")) return Promise.resolve(jsonRes(mockPositions));
    if (u.includes("/api/orders")) return Promise.resolve(jsonRes(mockOrders));
    return Promise.reject(new Error("unexpected"));
  });
});

describe("LivePage", () => {
  it("renders status cards after loading", async () => {
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getAllByText("AAPL").length).toBeGreaterThanOrEqual(1);
    });
    expect(screen.getAllByText("VF").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("1h")).toBeInTheDocument();
  });

  it("renders trades table", async () => {
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getByText("ENTRY")).toBeInTheDocument();
    });
    expect(screen.getAllByText("VF").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("bounce")).toBeInTheDocument();
  });

  it("renders IBKR positions", async () => {
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getByText("IBKR Positions")).toBeInTheDocument();
    });
    expect(screen.getByText("Avg Cost")).toBeInTheDocument();
  });

  it("renders IBKR orders", async () => {
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getByText("IBKR Orders")).toBeInTheDocument();
    });
    expect(screen.getByText("PreSubmitted")).toBeInTheDocument();
    expect(screen.getByText("STP")).toBeInTheDocument();
  });

  it("shows Loading for equity when zero", async () => {
    const zeroEquityStatus = {
      ...mockStatus,
      tickers: [{ ...mockStatus.tickers[0], equity: 0 }],
    };
    vi.spyOn(globalThis, "fetch").mockImplementation((url) => {
      const u = typeof url === "string" ? url : url.toString();
      const jsonRes = (body: unknown) =>
        new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
      if (u.includes("/api/status")) return Promise.resolve(jsonRes(zeroEquityStatus));
      if (u.includes("/api/trades")) return Promise.resolve(jsonRes([]));
      if (u.includes("/api/positions")) return Promise.resolve(jsonRes([]));
      if (u.includes("/api/orders")) return Promise.resolve(jsonRes([]));
      return Promise.reject(new Error("unexpected"));
    });
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getByText("Loading…")).toBeInTheDocument();
    });
  });

  it("shows error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("offline"));
    render(<LivePage />);
    await waitFor(() => {
      expect(screen.getByText(/Live server not connected/)).toBeInTheDocument();
    });
  });
});
