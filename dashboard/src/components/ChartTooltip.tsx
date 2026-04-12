interface ChartTooltipProps {
  x: number;
  y: number;
  lines: string[];
  maxWidth?: number;
  containerWidth?: number;
}

export function ChartTooltip({ x, y, lines, maxWidth = 700, containerWidth = 600 }: ChartTooltipProps) {
  return (
    <div
      style={{
        position: "absolute",
        left: Math.min(x + 12, containerWidth - maxWidth + 180),
        top: Math.max(y - 10, 0),
        background: "rgba(19, 23, 34, 0.95)",
        border: "1px solid #363a45",
        borderRadius: 4,
        padding: "6px 10px",
        fontSize: 11,
        lineHeight: 1.5,
        color: "#d1d4dc",
        pointerEvents: "none",
        zIndex: 100,
        whiteSpace: "pre",
        fontFamily: "monospace",
        maxWidth,
      }}
    >
      {lines.map((line, i) => (
        <div key={i} style={
          line.startsWith("\u2717") ? { color: "#ff6d00", fontWeight: 600 }
          : line.startsWith("\u2713") ? { color: "#26a69a", fontWeight: 600 }
          : undefined
        }>
          {line}
        </div>
      ))}
    </div>
  );
}
