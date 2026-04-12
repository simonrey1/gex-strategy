import type {
  IChartApi,
  ISeriesApi,
  ISeriesPrimitive,
  ISeriesPrimitivePaneRenderer,
  ISeriesPrimitivePaneView,
  SeriesType,
  Time,
} from "lightweight-charts";

export interface SpikeWindowSpec {
  start: Time;
  end: Time;
  color: string;
}

class SpikeWindowRenderer implements ISeriesPrimitivePaneRenderer {
  private _bands: { x1: number; x2: number; color: string }[];
  private _visible: boolean;
  constructor(bands: { x1: number; x2: number; color: string }[], visible: boolean) {
    this._bands = bands;
    this._visible = visible;
  }
  draw(target: Parameters<ISeriesPrimitivePaneRenderer["draw"]>[0]) {
    if (!this._visible) return;
    target.useBitmapCoordinateSpace((scope) => {
      const ctx = scope.context;
      const hr = scope.horizontalPixelRatio;
      const h = scope.bitmapSize.height;
      ctx.globalAlpha = 0.12;
      for (const { x1, x2, color } of this._bands) {
        const px1 = Math.round(x1 * hr);
        const px2 = Math.round(x2 * hr);
        const w = Math.max(px2 - px1, Math.round(2 * hr));
        ctx.fillStyle = color;
        ctx.fillRect(px1, 0, w, h);
      }
      ctx.globalAlpha = 1;
    });
  }
}

class SpikeWindowPaneView implements ISeriesPrimitivePaneView {
  private _source: SpikeWindowPrimitive;
  private _resolved: { x1: number; x2: number; color: string }[] = [];
  constructor(source: SpikeWindowPrimitive) {
    this._source = source;
  }
  update() {
    const ts = this._source._chart.timeScale();
    this._resolved = [];
    for (const spec of this._source._windows) {
      const x1 = ts.timeToCoordinate(spec.start);
      const x2 = ts.timeToCoordinate(spec.end);
      if (x1 !== null && x2 !== null) {
        this._resolved.push({ x1, x2, color: spec.color });
      }
    }
  }
  renderer() {
    return new SpikeWindowRenderer(this._resolved, this._source._visible);
  }
}

export class SpikeWindowPrimitive implements ISeriesPrimitive {
  _chart: IChartApi;
  _series: ISeriesApi<SeriesType>;
  _windows: SpikeWindowSpec[];
  _visible = true;
  private _paneViews: SpikeWindowPaneView[];

  constructor(chart: IChartApi, series: ISeriesApi<SeriesType>, windows: SpikeWindowSpec[]) {
    this._chart = chart;
    this._series = series;
    this._windows = windows;
    this._paneViews = [new SpikeWindowPaneView(this)];
  }

  setVisible(v: boolean) {
    this._visible = v;
    this._chart.timeScale().applyOptions({});
  }

  updateAllViews() {
    this._paneViews.forEach((v) => v.update());
  }
  paneViews() {
    return this._paneViews;
  }
}
