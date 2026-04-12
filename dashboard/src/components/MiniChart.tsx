import { useEffect, useRef, useState } from "react";
import { createChart, type IChartApi } from "lightweight-charts";
import type { BarTooltip, ChartBar, WallPoint } from "@shared/types";
import { shiftToET } from "../lib/format";
import { CHART_COLORS, CANDLE_COLORS } from "../lib/chartTheme";
import { useTooltipMap, subscribeCrosshairTooltip } from "../hooks/useTooltipMap";
import { ChartTooltip } from "./ChartTooltip";
import { SpikeWindowPrimitive } from "./SpikeWindowPrimitive";

interface MiniChartProps {
  bars: ChartBar[];
  smoothPutWall: WallPoint[];
  smoothCallWall: WallPoint[];
  entryTimeSec: number;
  entryPrice: number;
  spikeTooltips?: BarTooltip[];
  spikeStartSec?: number;
  height?: number;
}

export function MiniChart({
  bars, smoothPutWall, smoothCallWall,
  entryTimeSec, entryPrice, spikeTooltips, spikeStartSec, height = 200,
}: MiniChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const [tooltip, setTooltip] = useState<{ x: number; y: number; lines: string[] } | null>(null);

  const tooltipMapRef = useTooltipMap(spikeTooltips);

  useEffect(() => {
    if (!containerRef.current || bars.length === 0) return;

    const chart = createChart(containerRef.current, {
      width: containerRef.current.clientWidth,
      height,
      layout: { background: { color: CHART_COLORS.bg }, textColor: CHART_COLORS.text, fontSize: 10 },
      grid: { vertLines: { color: CHART_COLORS.grid }, horzLines: { color: CHART_COLORS.grid } },
      rightPriceScale: { borderColor: CHART_COLORS.border, scaleMargins: { top: 0.05, bottom: 0.05 } },
      timeScale: { borderColor: CHART_COLORS.border, timeVisible: true, secondsVisible: false, visible: false },
      crosshair: { mode: 0 },
      handleScroll: false,
      handleScale: false,
    });
    chartRef.current = chart;

    const candleSeries = chart.addCandlestickSeries(CANDLE_COLORS);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    candleSeries.setData(bars.map(b => ({ ...b, time: shiftToET(b.time) as any })));

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const markers: any[] = [];
    if (spikeStartSec && spikeStartSec > 0) {
      markers.push({
        time: shiftToET(spikeStartSec),
        position: "aboveBar",
        color: CHART_COLORS.spike,
        shape: "square",
        text: "spike",
      });
    }
    markers.push({
      time: shiftToET(entryTimeSec),
      position: "belowBar",
      color: "#f57c00",
      shape: "arrowUp",
      text: `$${entryPrice.toFixed(0)}`,
    });
    markers.sort((a: { time: number }, b: { time: number }) => a.time - b.time);
    candleSeries.setMarkers(markers);

    const tips = spikeTooltips ?? [];
    if (tips.length > 0) {
      const times = tips.map(t => t.time).sort((a, b) => a - b);
      candleSeries.attachPrimitive(new SpikeWindowPrimitive(chart, candleSeries, [{
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        start: shiftToET(times[0]!) as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        end: shiftToET(times[times.length - 1]!) as any,
        color: CHART_COLORS.spike,
      }]));
    }

    for (const [pts, color] of [[smoothPutWall, CHART_COLORS.positive], [smoothCallWall, CHART_COLORS.negative]] as const) {
      if (pts.length === 0) continue;
      const s = chart.addLineSeries({ color, lineWidth: 1, priceLineVisible: false, lastValueVisible: false });
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      s.setData(pts.map(p => ({ time: shiftToET(p.time) as any, value: p.value })));
    }

    subscribeCrosshairTooltip(chart, tooltipMapRef, setTooltip);

    chart.timeScale().fitContent();

    const ro = new ResizeObserver(() => {
      if (containerRef.current) {
        chart.applyOptions({ width: containerRef.current.clientWidth });
      }
    });
    ro.observe(containerRef.current);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
    };
  }, [bars, smoothPutWall, smoothCallWall, entryTimeSec, entryPrice, spikeStartSec, height]);

  return (
    <div ref={containerRef} style={{ width: "100%", height, position: "relative" }}>
      {tooltip && (
        <ChartTooltip
          x={tooltip.x}
          y={tooltip.y}
          lines={tooltip.lines}
          maxWidth={400}
          containerWidth={containerRef.current?.clientWidth ?? 400}
        />
      )}
    </div>
  );
}
