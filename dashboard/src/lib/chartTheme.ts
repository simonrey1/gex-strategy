import { CrosshairMode, LineStyle, type DeepPartial, type ChartOptions } from "lightweight-charts";

const BG       = "#0a0e17";
const GRID     = "#111827";
const TEXT     = "#787b86";
const BORDER   = "#1a1e2e";
const CROSSHAIR = "#3a3f52";

export const CHART_COLORS = {
  bg: BG,
  grid: GRID,
  text: TEXT,
  border: BORDER,
  crosshair: CROSSHAIR,
  positive: "#26a69a",
  negative: "#ef5350",
  baseline: "#363a45",
  spike: "#9c27b0",
} as const;

export const CANDLE_COLORS = {
  upColor: CHART_COLORS.positive,
  downColor: CHART_COLORS.negative,
  borderUpColor: CHART_COLORS.positive,
  borderDownColor: CHART_COLORS.negative,
  wickUpColor: CHART_COLORS.positive,
  wickDownColor: CHART_COLORS.negative,
} as const;

export function chartOptions(overrides?: DeepPartial<ChartOptions>): DeepPartial<ChartOptions> {
  return {
    layout: { background: { color: BG }, textColor: TEXT, fontSize: 11 },
    grid: { vertLines: { color: GRID }, horzLines: { color: GRID } },
    crosshair: {
      mode: CrosshairMode.Normal,
      vertLine: { color: CROSSHAIR, width: 1, style: LineStyle.Dashed },
      horzLine: { color: CROSSHAIR, width: 1, style: LineStyle.Dashed },
    },
    rightPriceScale: { borderColor: BORDER },
    timeScale: { borderColor: BORDER, timeVisible: true, secondsVisible: false, minBarSpacing: 0.2 },
    ...overrides,
  };
}
