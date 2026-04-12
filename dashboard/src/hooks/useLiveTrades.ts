import { useState, useEffect, useRef, useCallback } from "react";
import type { TradeRecord, LiveStatus, IbkrPosition, IbkrOrder } from "@shared/types";

const POLL_MS = 15_000;

export function useLiveTrades() {
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const [status, setStatus] = useState<LiveStatus | null>(null);
  const [positions, setPositions] = useState<IbkrPosition[] | null>(null);
  const [orders, setOrders] = useState<IbkrOrder[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const refresh = useCallback(async () => {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    const signal = ctrl.signal;

    try {
      const [statusRes, tradesRes, posRes, ordRes] = await Promise.all([
        fetch("/api/status", { signal }),
        fetch("/api/trades", { signal }),
        fetch("/api/positions", { signal }),
        fetch("/api/orders", { signal }),
      ]);

      if (signal.aborted) return;

      const isJson = (r: Response) =>
        (r.headers.get("content-type") ?? "").includes("application/json");

      if (!statusRes.ok || !tradesRes.ok || !isJson(statusRes) || !isJson(tradesRes)) {
        setError("Live server not connected");
        return;
      }

      const statusJson: unknown = await statusRes.json();
      const tradesJson: unknown = await tradesRes.json();

      if (signal.aborted) return;

      setStatus(statusJson as LiveStatus);
      setTrades(tradesJson as TradeRecord[]);

      if (posRes.ok && isJson(posRes)) {
        setPositions((await posRes.json()) as IbkrPosition[]);
      }
      if (ordRes.ok && isJson(ordRes)) {
        setOrders((await ordRes.json()) as IbkrOrder[]);
      }

      setError(null);
    } catch (e) {
      if (signal.aborted) return;
      setError("Live server not connected");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), POLL_MS);
    return () => {
      clearInterval(id);
      abortRef.current?.abort();
    };
  }, [refresh]);

  return { trades, status, positions, orders, error, refresh };
}
