import type {
  IChartApi,
  ISeriesApi,
  ISeriesPrimitive,
  ISeriesPrimitivePaneRenderer,
  ISeriesPrimitivePaneView,
  SeriesType,
  Time,
} from "lightweight-charts";
import type { WallBandBar } from "@shared/types";

const MAX_HEIGHT_PX = 24;

class WallBandRenderer implements ISeriesPrimitivePaneRenderer {
  private _data: WallBandBar[];
  private _chart: IChartApi;
  private _series: ISeriesApi<SeriesType>;
  private _showPut: boolean;
  private _showCall: boolean;

  constructor(
    data: WallBandBar[],
    chart: IChartApi,
    series: ISeriesApi<SeriesType>,
    showPut: boolean,
    showCall: boolean,
  ) {
    this._data = data;
    this._chart = chart;
    this._series = series;
    this._showPut = showPut;
    this._showCall = showCall;
  }

  draw(target: Parameters<ISeriesPrimitivePaneRenderer["draw"]>[0]) {
    target.useBitmapCoordinateSpace((scope) => {
      const ctx = scope.context;
      const hr = scope.horizontalPixelRatio;
      const vr = scope.verticalPixelRatio;
      const ts = this._chart.timeScale();
      const series = this._series;
      const data = this._data;
      if (data.length === 0) return;

      const visRange = ts.getVisibleLogicalRange();
      if (!visRange) return;

      const barsCount = data.length;
      const coordRange = ts.getVisibleRange();
      if (!coordRange) return;
      const tFrom = (coordRange.from as number);
      const tTo = (coordRange.to as number);

      // Binary search for first visible bar
      let lo = 0, hi = barsCount;
      while (lo < hi) {
        const mid = (lo + hi) >>> 1;
        if (data[mid]!.time < tFrom) lo = mid + 1; else hi = mid;
      }
      const start = Math.max(0, lo - 1);

      const barSpacing = ts.options().barSpacing ?? 6;
      const halfBar = (barSpacing * hr) / 2;
      const canvasW = scope.bitmapSize.width;

      // Pre-compute alpha LUT (10 buckets)
      const alphaLut: string[] = [];
      for (let i = 0; i <= 10; i++) {
        const pct = i / 10;
        alphaLut.push((0.35 + pct * 0.50).toFixed(2));
      }

      const putR = 41, putG = 98, putB = 255;
      const callR = 244, callG = 67, callB = 54;

      for (let i = start; i < barsCount; i++) {
        const bar = data[i]!;
        if (bar.time > tTo) break;

        const x = ts.timeToCoordinate(bar.time as Time);
        if (x === null) continue;
        const cx = x * hr;
        if (cx + halfBar < 0 || cx - halfBar > canvasW) continue;

        const x0 = cx - halfBar;
        const w = halfBar * 2;

        const drawSide = (walls: typeof bar.putWalls, r: number, g: number, b: number) => {
          for (const wall of walls) {
            const y = series.priceToCoordinate(wall.strike);
            if (y === null) continue;
            const cy = y * vr;
            const h = Math.max(2 * vr, wall.pct * MAX_HEIGHT_PX * vr);
            const ai = Math.min(10, (wall.pct * 10) | 0);
            ctx.fillStyle = `rgba(${r},${g},${b},${alphaLut[ai]})`;
            ctx.fillRect(x0, cy - h / 2, w, h);
          }
        };

        if (this._showPut) drawSide(bar.putWalls, putR, putG, putB);
        if (this._showCall) drawSide(bar.callWalls, callR, callG, callB);
      }
    });
  }
}

class WallBandPaneView implements ISeriesPrimitivePaneView {
  private _source: WallBandPrimitive;
  constructor(source: WallBandPrimitive) {
    this._source = source;
  }
  zOrder(): "bottom" {
    return "bottom";
  }
  renderer(): WallBandRenderer | null {
    const s = this._source;
    if (!s._chart || !s._series) return null;
    return new WallBandRenderer(s._data, s._chart, s._series, s._showPut, s._showCall);
  }
}

export class WallBandPrimitive implements ISeriesPrimitive<Time> {
  _chart: IChartApi | null = null;
  _series: ISeriesApi<SeriesType> | null = null;
  _requestUpdate: (() => void) | null = null;
  _data: WallBandBar[] = [];
  _showPut = true;
  _showCall = true;

  private _view = new WallBandPaneView(this);

  setData(data: WallBandBar[]) {
    this._data = data;
    this._requestUpdate?.();
  }

  setVisibility(showPut: boolean, showCall: boolean) {
    if (this._showPut === showPut && this._showCall === showCall) return;
    this._showPut = showPut;
    this._showCall = showCall;
    this._requestUpdate?.();
  }

  attached(params: {
    chart: IChartApi;
    series: ISeriesApi<SeriesType>;
    requestUpdate: () => void;
  }) {
    this._chart = params.chart;
    this._series = params.series;
    this._requestUpdate = params.requestUpdate;
  }

  detached() {
    this._chart = null;
    this._series = null;
    this._requestUpdate = null;
  }

  paneViews() {
    return [this._view];
  }
}
