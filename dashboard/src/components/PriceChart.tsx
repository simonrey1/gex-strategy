import { useEffect, useMemo, useRef, useState } from "react";
import {
  createChart,
  type IChartApi,
  type ISeriesApi,
} from "lightweight-charts";
import type { ChartData } from "@shared/types";
import { shiftToET } from "../lib/format";
import { useTooltipMap, subscribeCrosshairTooltip } from "../hooks/useTooltipMap";
import { chartOptions, CHART_COLORS, CANDLE_COLORS } from "../lib/chartTheme";
import { WALL_DEFS, WALL_GROUPS, GROUPED_WALL_KEYS, type WallKey } from "../types";
import { WallBandPrimitive } from "./WallBandPrimitive";
import { SpikeWindowPrimitive } from "./SpikeWindowPrimitive";
import { ChartTooltip } from "./ChartTooltip";

interface PriceChartProps {
  data: ChartData;
  hiddenSeries?: Set<WallKey>;
  showSpikeWindows?: boolean;
  onChartReady?: (chart: IChartApi) => void;
}

const PW_GROUP = WALL_GROUPS.find((g) => g.id === "pw");
const CW_GROUP = WALL_GROUPS.find((g) => g.id === "cw");

function bandVisibility(hidden?: Set<WallKey>): [boolean, boolean] {
  return [
    !PW_GROUP?.keys.every((k) => hidden?.has(k)),
    !CW_GROUP?.keys.every((k) => hidden?.has(k)),
  ];
}

export function PriceChart({ data, hiddenSeries, showSpikeWindows = true, onChartReady }: PriceChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const wallSeriesRef = useRef<Map<WallKey, ISeriesApi<"Line">>>(new Map());
  const bandPrimitiveRef = useRef<WallBandPrimitive | null>(null);
  const spikePrimitiveRef = useRef<SpikeWindowPrimitive | null>(null);
  const onChartReadyRef = useRef(onChartReady);
  onChartReadyRef.current = onChartReady;
  const [tooltip, setTooltip] = useState<{ x: number; y: number; lines: string[] } | null>(null);
  const [copyToast, setCopyToast] = useState<{ x: number; y: number } | null>(null);

  const etData = useMemo(() => {
    const s = shiftToET;
    const shiftWall = (pts: typeof data.putWalls) => pts.map((p) => ({ ...p, time: s(p.time) }));
    const shiftBands = (bars: NonNullable<typeof data.wallBands>) =>
      bars.map((b) => ({ ...b, time: s(b.time) }));
    return {
      ...data,
      bars: data.bars.map((b) => ({ ...b, time: s(b.time) })),
      markers: data.markers.map((m) => ({ ...m, time: s(m.time) })),
      putWalls: shiftWall(data.putWalls),
      callWalls: shiftWall(data.callWalls),
      midPutWall: shiftWall(data.midPutWall ?? []),
      widePutWall: shiftWall(data.widePutWall ?? []),
      wideCallWall: shiftWall(data.wideCallWall ?? []),
      smoothPutWall: shiftWall(data.smoothPutWall ?? []),
      smoothCallWall: shiftWall(data.smoothCallWall ?? []),
      smoothHighestPw: shiftWall(data.smoothHighestPw ?? []),
      smoothLowestCw: shiftWall(data.smoothLowestCw ?? []),
      spreadPutWall: shiftWall(data.spreadPutWall ?? []),
      spreadCallWall: shiftWall(data.spreadCallWall ?? []),
      ivEmaFast: shiftWall(data.ivEmaFast ?? []),
      ivEmaSlow: shiftWall(data.ivEmaSlow ?? []),
      wallBands: shiftBands(data.wallBands ?? []),
    };
  }, [data]);

  const tooltipMapRef = useTooltipMap(data.spikeTooltips);

  const hiddenRef = useRef(hiddenSeries);
  hiddenRef.current = hiddenSeries;

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const chart = createChart(el, {
      ...chartOptions({
        rightPriceScale: { scaleMargins: { top: 0.05, bottom: 0.05 } },
      }),
      width: el.clientWidth,
      height: el.clientHeight,
    });
    chartRef.current = chart;

    const candles = chart.addCandlestickSeries({
      ...CANDLE_COLORS,
      wickUpColor: "#4caf7a",
      wickDownColor: "#e57373",
      priceLineVisible: false,
      lastValueVisible: false,
    });
    candles.setData(etData.bars as Parameters<typeof candles.setData>[0]);

    const markers = etData.markers.map((m) => ({ ...m, size: m.size ?? 2 }));
    candles.setMarkers(markers as Parameters<typeof candles.setMarkers>[0]);

    // Wall band primitive (proportional gamma rendering)
    const bandPrimitive = new WallBandPrimitive();
    bandPrimitive.setData(etData.wallBands);
    bandPrimitive.setVisibility(...bandVisibility(hiddenRef.current));
    candles.attachPrimitive(bandPrimitive);
    bandPrimitiveRef.current = bandPrimitive;

    const spikeWins = data.spikeWindows ?? [];
    if (spikeWins.length > 0) {
      const specs = spikeWins.map(w => ({
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        start: shiftToET(w.start) as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        end: shiftToET(w.end) as any,
        color: CHART_COLORS.spike,
      }));
      const sp = new SpikeWindowPrimitive(chart, candles, specs);
      candles.attachPrimitive(sp);
      spikePrimitiveRef.current = sp;
    }

    const IV_KEYS: Set<string> = new Set(["ivEmaFast", "ivEmaSlow"]);

    // Non-grouped wall line series (smooth, wide, weekly, mid, IV)
    const seriesMap = new Map<WallKey, ISeriesApi<"Line">>();
    for (const w of WALL_DEFS) {
      if (GROUPED_WALL_KEYS.has(w.key)) continue;
      const pts = (etData[w.key] ?? []) as typeof etData.putWalls;
      if (pts.length === 0) continue;
      const isIv = IV_KEYS.has(w.key);
      const s = chart.addLineSeries({
        color: w.color,
        lineWidth: w.width,
        lineStyle: w.style,
        crosshairMarkerVisible: false,
        lastValueVisible: false,
        priceLineVisible: false,
        title: w.title,
        visible: !hiddenRef.current?.has(w.key),
        ...(isIv ? { priceScaleId: "iv" } : {}),
      });
      s.setData(pts as Parameters<typeof s.setData>[0]);
      seriesMap.set(w.key, s);
    }
    // Hide the IV price scale labels (visible only when IV series toggled on)
    chart.priceScale("iv").applyOptions({
      scaleMargins: { top: 0.7, bottom: 0.05 },
      borderVisible: false,
    });
    wallSeriesRef.current = seriesMap;

    subscribeCrosshairTooltip(chart, tooltipMapRef, setTooltip);

    chart.subscribeClick((param) => {
      if (!param.time || !param.point) return;
      const lines = tooltipMapRef.current.get(param.time as number);
      if (!lines) return;
      void navigator.clipboard.writeText(lines.join("\n")).then(() => {
        setCopyToast({ x: param.point!.x, y: param.point!.y });
        setTimeout(() => setCopyToast(null), 1000);
      });
    });

    chart.timeScale().fitContent();
    onChartReadyRef.current?.(chart);

    const ro = new ResizeObserver(() => {
      chart.applyOptions({ width: el.clientWidth });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      wallSeriesRef.current = new Map();
      bandPrimitiveRef.current = null;
      spikePrimitiveRef.current = null;
    };
  }, [etData]);

  useEffect(() => {
    for (const [key, series] of wallSeriesRef.current) {
      series.applyOptions({ visible: !hiddenSeries?.has(key) });
    }
    bandPrimitiveRef.current?.setVisibility(...bandVisibility(hiddenSeries));
  }, [hiddenSeries]);

  useEffect(() => {
    spikePrimitiveRef.current?.setVisible(showSpikeWindows);
  }, [showSpikeWindows]);

  return (
    <div ref={containerRef} style={{ width: "100%", flex: 1, minHeight: 0, position: "relative" }}>
      {tooltip && (
        <ChartTooltip
          x={tooltip.x}
          y={tooltip.y}
          lines={tooltip.lines}
          containerWidth={containerRef.current?.clientWidth ?? 600}
        />
      )}
      {copyToast && (
        <div
          style={{
            position: "absolute",
            left: copyToast.x + 12,
            top: copyToast.y - 24,
            background: "#4caf50",
            color: "#fff",
            borderRadius: 4,
            padding: "3px 8px",
            fontSize: 11,
            fontWeight: 600,
            pointerEvents: "none",
            zIndex: 20,
          }}
        >
          Copied
        </div>
      )}
    </div>
  );
}
