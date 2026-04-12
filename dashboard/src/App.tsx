import { useState, useEffect, useCallback } from "react";
import { LivePage } from "./pages/LivePage";
import { BacktestPage } from "./pages/BacktestPage";

type Tab = "live" | "backtest";

function getInitialTab(): Tab {
  const params = new URLSearchParams(window.location.search);
  const t = params.get("tab");
  return t === "backtest" ? "backtest" : "live";
}

export function App() {
  const [tab, setTab] = useState<Tab>(getInitialTab);

  const switchTab = useCallback((t: Tab) => {
    setTab(t);
    const url = new URL(window.location.href);
    url.searchParams.set("tab", t);
    window.history.replaceState(null, "", url.toString());
  }, []);

  useEffect(() => {
    const onPop = () => setTab(getInitialTab());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", overflow: "hidden" }}>
      <nav className="nav">
        <span className="nav-title">GEX Strategy</span>
        <button
          className={`nav-tab ${tab === "live" ? "active" : ""}`}
          onClick={() => switchTab("live")}
        >
          Live
        </button>
        <button
          className={`nav-tab ${tab === "backtest" ? "active" : ""}`}
          onClick={() => switchTab("backtest")}
        >
          Backtest
        </button>
      </nav>
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, overflowY: "auto" }}>
        {tab === "live" ? <LivePage /> : <BacktestPage />}
      </div>
    </div>
  );
}
