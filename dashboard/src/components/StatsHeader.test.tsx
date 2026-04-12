import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { StatsHeader } from "./StatsHeader";
import type { BacktestResult } from "../types";

const result: BacktestResult = {
  ticker: "AAPL",
  label: "AAPL",
  startDate: "2024-11-01",
  endDate: "2025-01-31",
  totalBars: 5000,
  totalTrades: 42,
  winners: 25,
  losers: 17,
  winRate: 0.595,
  grossPnl: 1200,
  totalCommission: 84,
  totalSlippage: 42,
  netPnl: 1074,
  totalReturnPct: 10.74,
  profitFactor: 2.1,
  maxDrawdown: 500,
  maxDrawdownPct: 0.05,
  sharpeRatio: 1.45,
  sortinoRatio: 2.1,
  calmarRatio: 0.5,
  cagr: 0.12,
  expectancy: 25.57,
  payoffRatio: 1.55,
  ulcerIndex: 0.02,
  maxDdDurationDays: 30,
  avgTradeDurationMinutes: 120,
  avgWinPct: 0.85,
  avgLossPct: -0.55,
  avgTradePct: 0.26,
  buyHoldReturnPct: 8.5,
  alphaPct: 2.24,
  monthlyReturns: [],
  trades: [],
  tradeAnalysis: { highRunupLosers: [], worstLosses: [] },
  wallEvents: [],
  avgCapitalUtilPct: 0.5,
};

describe("StatsHeader", () => {
  it("renders ticker and date range", () => {
    render(<StatsHeader r={result} />);
    expect(screen.getByText(/AAPL/)).toBeInTheDocument();
    expect(screen.getByText(/2024-11-01/)).toBeInTheDocument();
  });

  it("renders strategy vs buy & hold with alpha", () => {
    render(<StatsHeader r={result} />);
    expect(screen.getByText("+10.74%")).toBeInTheDocument();
    expect(screen.getByText("+8.50%")).toBeInTheDocument();
    expect(screen.getByText("+2.24%")).toBeInTheDocument();
    expect(screen.getByText(/\+\$1074 net/)).toBeInTheDocument();
  });

  it("renders trade count with W/L breakdown", () => {
    render(<StatsHeader r={result} />);
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText(/25W \/ 17L/)).toBeInTheDocument();
  });

  it("renders profit factor", () => {
    render(<StatsHeader r={result} />);
    expect(screen.getByText("2.10")).toBeInTheDocument();
  });

  it("renders PnL bar when gross > 0", () => {
    const { container } = render(<StatsHeader r={result} />);
    expect(container.querySelector(".pnl-bar")).toBeInTheDocument();
  });

  it("does not render PnL bar when gross <= 0", () => {
    const { container } = render(<StatsHeader r={{ ...result, grossPnl: -100 }} />);
    expect(container.querySelector(".pnl-bar")).not.toBeInTheDocument();
  });
});
