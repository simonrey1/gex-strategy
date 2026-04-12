import { WALL_DEFS, WALL_GROUPS, GROUPED_WALL_KEYS, type WallKey } from "../types";
import { CHART_COLORS } from "../lib/chartTheme";

interface ChartLegendProps {
  hidden: Set<WallKey>;
  onToggle: (key: WallKey) => void;
  onToggleGroup: (keys: WallKey[]) => void;
  showSpikeWindows?: boolean;
  onToggleSpikeWindows?: () => void;
  hasSpikeWindows?: boolean;
}

export function ChartLegend({
  hidden, onToggle, onToggleGroup,
  showSpikeWindows = true, onToggleSpikeWindows, hasSpikeWindows,
}: ChartLegendProps) {
  return (
    <div className="legend">
      {WALL_GROUPS.map((g) => {
        const allHidden = g.keys.every((k) => hidden.has(k));
        return (
          <div
            key={g.id}
            className={`legend-item ${allHidden ? "legend-off" : ""}`}
            onClick={() => onToggleGroup(g.keys)}
          >
            <div className="legend-line" style={{ background: allHidden ? "#555" : g.color }} />
            {g.title}
          </div>
        );
      })}
      {WALL_DEFS.filter((w) => !GROUPED_WALL_KEYS.has(w.key)).map((item) => {
        const off = hidden.has(item.key);
        return (
          <div
            key={item.key}
            className={`legend-item ${off ? "legend-off" : ""}`}
            onClick={() => onToggle(item.key)}
          >
            <div
              className="legend-line"
              style={{
                background: off ? "#555" : item.color,
                ...(item.dashed ? { backgroundImage: `repeating-linear-gradient(90deg, ${off ? "#555" : item.color} 0 4px, transparent 4px 8px)`, background: "transparent" } : {}),
              }}
            />
            {item.title}
          </div>
        );
      })}
      {hasSpikeWindows && onToggleSpikeWindows && (
        <div
          className={`legend-item ${showSpikeWindows ? "" : "legend-off"}`}
          onClick={onToggleSpikeWindows}
        >
          <div
            className="legend-line"
            style={{
              background: showSpikeWindows ? CHART_COLORS.spike : "#555",
              opacity: showSpikeWindows ? 0.5 : 1,
            }}
          />
          Spike Windows
        </div>
      )}
    </div>
  );
}
