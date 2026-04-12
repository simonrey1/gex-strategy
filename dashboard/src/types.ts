export type {
  Signal,
  TradeDiagnostics,
  BacktestTrade,
  BacktestResult,
  SavedBacktestState,
  TradeRecord,
  LiveStatus,
  LiveTickerStatus,
  GexStreamStatus,
  GexPhase,
  ChartBar,
  WallPoint,
  ChartMarker,
  ChartData,
  IbkrPosition,
  IbkrOrder,
  TickerIndicators,
} from "@shared/types";

export {
  ALL_TICKERS_SYMBOL,
} from "@shared/types";

import { LineStyle } from "lightweight-charts";

// ── Wall series definitions (single source of truth) ─────────────────────────

export const WALL_DEFS = [
  { key: "putWalls",       color: "#2962ff",   width: 4 as const, style: LineStyle.Solid,  title: "PW 1",   dashed: false, group: "pw" },
  { key: "putWall2",       color: "#2962ffDD", width: 3 as const, style: LineStyle.Solid,  title: "PW 2",   dashed: false, group: "pw" },
  { key: "putWall3",       color: "#2962ffAA", width: 2 as const, style: LineStyle.Solid,  title: "PW 3",   dashed: false, group: "pw" },
  { key: "putWall4",       color: "#2962ff77", width: 2 as const, style: LineStyle.Solid,  title: "PW 4",   dashed: false, group: "pw" },
  { key: "putWall5",       color: "#2962ff44", width: 1 as const, style: LineStyle.Solid,  title: "PW 5",   dashed: false, group: "pw" },
  { key: "callWalls",      color: "#f44336",   width: 4 as const, style: LineStyle.Solid,  title: "CW 1",   dashed: false, group: "cw" },
  { key: "callWall2",      color: "#f44336DD", width: 3 as const, style: LineStyle.Solid,  title: "CW 2",   dashed: false, group: "cw" },
  { key: "callWall3",      color: "#f44336AA", width: 2 as const, style: LineStyle.Solid,  title: "CW 3",   dashed: false, group: "cw" },
  { key: "callWall4",      color: "#f4433677", width: 2 as const, style: LineStyle.Solid,  title: "CW 4",   dashed: false, group: "cw" },
  { key: "callWall5",      color: "#f4433644", width: 1 as const, style: LineStyle.Solid,  title: "CW 5",   dashed: false, group: "cw" },
  { key: "midPutWall",     color: "#42a5f5", width: 1 as const, style: LineStyle.Solid,  title: "Mid PW",   dashed: false, group: null },
  { key: "widePutWall",    color: "#2962ff", width: 1 as const, style: LineStyle.Dashed, title: "Struct PW", dashed: true,  group: null },
  { key: "wideCallWall",   color: "#f44336", width: 1 as const, style: LineStyle.Dashed, title: "Struct CW", dashed: true,  group: null },
  { key: "smoothPutWall",    color: "#00e5ff", width: 2 as const, style: LineStyle.Solid,  title: "Sm PW",      dashed: false, group: null },
  { key: "smoothCallWall",   color: "#ff4081", width: 2 as const, style: LineStyle.Solid,  title: "Sm CW",      dashed: false, group: null },
  { key: "smoothHighestPw",  color: "#76ff03", width: 2 as const, style: LineStyle.Solid,  title: "Sm Hi PW",   dashed: false, group: null },
  { key: "smoothLowestCw",  color: "#e040fb", width: 2 as const, style: LineStyle.Solid,  title: "Sm Lo CW",   dashed: false, group: null },
  { key: "spreadPutWall",    color: "#00e5ff", width: 1 as const, style: LineStyle.Dashed, title: "Spread PW",  dashed: true,  group: null },
  { key: "spreadCallWall",   color: "#ff4081", width: 1 as const, style: LineStyle.Dashed, title: "Spread CW",  dashed: true,  group: null },
  { key: "ivEmaFast",        color: "#ff9800", width: 1 as const, style: LineStyle.Solid,  title: "IV Fast",    dashed: false, group: null },
  { key: "ivEmaSlow",        color: "#42a5f5", width: 1 as const, style: LineStyle.Solid,  title: "IV Slow",    dashed: false, group: null },
] as const;

export const WALL_GROUPS: { id: string; title: string; color: string; keys: WallKey[] }[] = [
  { id: "pw", title: "Put Walls", color: "#2962ff", keys: WALL_DEFS.filter(d => d.group === "pw").map(d => d.key) },
  { id: "cw", title: "Call Walls", color: "#f44336", keys: WALL_DEFS.filter(d => d.group === "cw").map(d => d.key) },
];

export type WallKey = (typeof WALL_DEFS)[number]["key"];

export const GROUPED_WALL_KEYS: Set<WallKey> = new Set(WALL_GROUPS.flatMap(g => g.keys));

// ── General UI types ─────────────────────────────────────────────────────────

export interface Column<T> {
  key: string;
  header: string;
  render: (row: T, index: number) => React.ReactNode;
  className?: string;
}

import type { Signal as SignalType } from "@shared/types";
import type { BadgeVariant } from "./components/Badge";

export const SIGNAL_FLAT = "FLAT" as const;

export const SIGNAL_BADGE: Record<Exclude<SignalType, "FLAT">, { label: string; variant: BadgeVariant }> = {
  LONG_VANNA_FLIP: { label: "VF", variant: "vf" },
  LONG_WALL_BOUNCE: { label: "WB", variant: "wb" },
};

const ENTRY_LABELS = Object.values(SIGNAL_BADGE).map((b) => b.label);

/** True if the marker text starts with any known entry signal label (VF, WB, …). */
export function isEntryMarkerText(text: string): boolean {
  return ENTRY_LABELS.some((l) => text.startsWith(l));
}
