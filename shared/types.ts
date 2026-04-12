import type { ChartBar as GenChartBar } from "../bindings/shared/generated/ChartBar";
import type { TradeDiagnostics as GenTradeDiagnostics } from "../bindings/shared/generated/TradeDiagnostics";
import type { Trade as GenTrade } from "../bindings/shared/generated/Trade";
import type { BacktestResult as GenBacktestResult } from "../bindings/shared/generated/BacktestResult";
import type { TradeRecord as GenTradeRecord } from "../bindings/shared/generated/TradeRecord";
import type { LiveStatus as GenLiveStatus } from "../bindings/shared/generated/LiveStatus";
import type { LiveTickerStatus as GenLiveTickerStatus } from "../bindings/shared/generated/LiveTickerStatus";
import type { GexStreamStatus as GenGexStreamStatus } from "../bindings/shared/generated/GexStreamStatus";
import type { GexPhase as GenGexPhase } from "../bindings/shared/generated/GexPhase";
import type { WallPoint as GenWallPoint } from "../bindings/shared/generated/WallPoint";
import type { ChartMarker as GenChartMarker } from "../bindings/shared/generated/ChartMarker";
import type { ChartData as GenChartData } from "../bindings/shared/generated/ChartData";
import type { IbkrPosition as GenIbkrPosition } from "../bindings/shared/generated/IbkrPosition";
import type { IbkrOrder as GenIbkrOrder } from "../bindings/shared/generated/IbkrOrder";
import type { Signal as GenSignal } from "../bindings/shared/generated/Signal";
import type { TickerIndicators as GenTickerIndicators } from "../bindings/shared/generated/TickerIndicators";
import type { IvScanResult as GenIvScanResult } from "../bindings/shared/generated/IvScanResult";
import type { ScanBucket as GenScanBucket } from "../bindings/shared/generated/ScanBucket";
import type { ScanSnapshot as GenScanSnapshot } from "../bindings/shared/generated/ScanSnapshot";
import type { WallBand as GenWallBand } from "../bindings/shared/generated/WallBand";
import type { WallBandBar as GenWallBandBar } from "../bindings/shared/generated/WallBandBar";
import type { BarTooltip as GenBarTooltip } from "../bindings/shared/generated/BarTooltip";
import type { MissedEntry as GenMissedEntry } from "../bindings/shared/generated/MissedEntry";
import type { MissedGateSummary as GenMissedGateSummary } from "../bindings/shared/generated/MissedGateSummary";
import type { MissedEntriesReport as GenMissedEntriesReport } from "../bindings/shared/generated/MissedEntriesReport";
import type { SpikeWindow as GenSpikeWindow } from "../bindings/shared/generated/SpikeWindow";

export const ALL_TICKERS_SYMBOL = "ALL" as const;

// ── Utility: bigint → number ─────────────────────────────────────────────────
// ts-rs maps Rust i64/u64 to TS bigint, but JSON.parse always returns number.
// This recursive mapped type normalises generated types for the JSON world.

type Numberify<T> =
  T extends bigint ? number :
  T extends (infer U)[] ? Numberify<U>[] :
  T extends object ? { [K in keyof T]: Numberify<T[K]> } :
  T;

// ── Canonical types (generated from Rust via ts-rs, bigint → number) ─────────

export type Signal = GenSignal;
export type TradeDiagnostics = Numberify<GenTradeDiagnostics>;
export type BacktestTrade = Numberify<GenTrade>;
export type BacktestResult = Numberify<Omit<GenBacktestResult, "ticker"> & { ticker: string }>;
export type IvScanResult = Numberify<GenIvScanResult>;
export type ScanBucket = GenScanBucket;
export type ScanSnapshot = Numberify<GenScanSnapshot>;
export type SavedBacktestState = {
  version: number;
  savedAt: string;
  result: BacktestResult;
  chartData?: ChartData | null;
  ivScan?: IvScanResult[] | null;
};
export type TradeRecord = Numberify<GenTradeRecord>;
export type LiveStatus = Numberify<GenLiveStatus>;
export type LiveTickerStatus = Numberify<GenLiveTickerStatus>;
export type GexStreamStatus = Numberify<GenGexStreamStatus>;
export type GexPhase = GenGexPhase;
export type ChartBar = Numberify<GenChartBar>;
export type WallPoint = Numberify<GenWallPoint>;
export type ChartMarker = Numberify<Omit<GenChartMarker, "size"> & { size?: number | null }>;
export type ChartData = Numberify<Omit<GenChartData, "markers"> & { markers: ChartMarker[] }>;
export type IbkrPosition = GenIbkrPosition;
export type IbkrOrder = GenIbkrOrder;
export type TickerIndicators = GenTickerIndicators;
export type WallBand = Numberify<GenWallBand>;
export type WallBandBar = Numberify<GenWallBandBar>;
export type BarTooltip = Numberify<GenBarTooltip>;
export type MissedEntry = Numberify<GenMissedEntry>;
export type MissedGateSummary = Numberify<GenMissedGateSummary>;
export type MissedEntriesReport = Numberify<GenMissedEntriesReport>;
export type SpikeWindow = Numberify<GenSpikeWindow>;

