import { type Dispatch, type MutableRefObject, type SetStateAction, useMemo, useRef } from "react";
import type { IChartApi, MouseEventParams } from "lightweight-charts";
import type { BarTooltip } from "@shared/types";
import { shiftToET } from "../lib/format";

type TooltipState = { x: number; y: number; lines: string[] } | null;

export function useTooltipMap(tips: BarTooltip[] | undefined) {
  const map = useMemo(() => {
    const m = new Map<number, string[]>();
    for (const t of tips ?? []) {
      m.set(shiftToET(t.time), t.lines);
    }
    return m;
  }, [tips]);

  const ref = useRef(map);
  ref.current = map;

  return ref;
}

export function subscribeCrosshairTooltip(
  chart: IChartApi,
  mapRef: MutableRefObject<Map<number, string[]>>,
  setTooltip: Dispatch<SetStateAction<TooltipState>>,
) {
  chart.subscribeCrosshairMove((param: MouseEventParams) => {
    if (!param.time || !param.point) { setTooltip(null); return; }
    const lines = mapRef.current.get(param.time as number);
    setTooltip(lines ? { x: param.point.x, y: param.point.y, lines } : null);
  });
}
