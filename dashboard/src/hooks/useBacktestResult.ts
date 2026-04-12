import { useState, useEffect, useCallback, useRef } from "react";
import { z } from "zod";
import type { BacktestResult, ChartData, SavedBacktestState } from "@shared/types";

interface CachedEntry {
  result: BacktestResult;
  chartData: ChartData | null;
}

function getTickerFromUrl(): string | null {
  const params = new URLSearchParams(window.location.search);
  return params.get("ticker");
}

function setTickerInUrl(ticker: string | null) {
  const params = new URLSearchParams(window.location.search);
  if (ticker) {
    params.set("ticker", ticker);
  } else {
    params.delete("ticker");
  }
  const qs = params.toString();
  const url = qs ? `${window.location.pathname}?${qs}` : window.location.pathname;
  window.history.replaceState(null, "", url);
}

export function useBacktestResult() {
  const [tickers, setTickers] = useState<string[]>([]);
  const [activeTicker, setActiveTicker] = useState<string | null>(() => getTickerFromUrl());
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [chartData, setChartData] = useState<ChartData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const cache = useRef(new Map<string, CachedEntry>());

  const applyEntry = useCallback((entry: CachedEntry) => {
    setResult(entry.result);
    setChartData(entry.chartData);
    setError(null);
  }, []);

  const fetchTickers = useCallback(async () => {
    try {
      const res = await fetch("/api/backtest/tickers");
      if (!res.ok) {
        setLoading(false);
        return;
      }
      const json: unknown = await res.json();
      const parsed = z.array(z.string()).safeParse(json);
      if (parsed.success && parsed.data.length > 0) {
        setTickers(parsed.data);
        setActiveTicker((prev) => {
          if (prev && parsed.data.includes(prev)) return prev;
          const fromUrl = getTickerFromUrl();
          if (fromUrl && parsed.data.includes(fromUrl)) return fromUrl;
          return parsed.data[0]!;
        });
      } else {
        setLoading(false);
      }
    } catch {
      setLoading(false);
    }
  }, []);

  const fetchResult = useCallback(async (ticker: string | null) => {
    const key = ticker ?? "__default__";
    const cached = cache.current.get(key);
    if (cached) {
      applyEntry(cached);
      setLoading(false);
      return;
    }

    setLoading(true);
    try {
      const qs = ticker ? `?ticker=${ticker}` : "";
      const res = await fetch(`/api/backtest${qs}`);
      if (!res.ok) {
        setError(`Failed to load backtest: ${res.status}`);
        setLoading(false);
        return;
      }
      const json: unknown = await res.json();
      const data = json as SavedBacktestState;

      const entry: CachedEntry = {
        result: data.result as BacktestResult,
        chartData: (data.chartData ?? null) as ChartData | null,
      };
      cache.current.set(key, entry);
      applyEntry(entry);
    } catch (e) {
      setError(e instanceof Error ? e.message : "fetch failed");
    } finally {
      setLoading(false);
    }
  }, [applyEntry]);

  useEffect(() => {
    void fetchTickers();
  }, [fetchTickers]);

  useEffect(() => {
    if (activeTicker != null) {
      setTickerInUrl(activeTicker);
      void fetchResult(activeTicker);
    }
  }, [activeTicker, fetchResult]);

  const selectTicker = useCallback((ticker: string) => {
    setActiveTicker(ticker);
    setTickerInUrl(ticker);
  }, []);

  const loadFromFile = useCallback((file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const json: unknown = JSON.parse(reader.result as string);
        const data = json as SavedBacktestState;
        const entry: CachedEntry = {
          result: data.result as BacktestResult,
          chartData: (data.chartData ?? null) as ChartData | null,
        };
        cache.current.set("__file__", entry);
        applyEntry(entry);
      } catch {
        setError("Invalid JSON file");
      }
    };
    reader.readAsText(file);
  }, [applyEntry]);

  return {
    tickers,
    activeTicker,
    selectTicker,
    result,
    chartData,
    loading,
    error,
    refresh: () => {
      if (activeTicker) cache.current.delete(activeTicker);
      return fetchResult(activeTicker);
    },
    loadFromFile,
  };
}
