import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ChartTooltip } from "./ChartTooltip";

describe("ChartTooltip", () => {
  it("renders all lines", () => {
    render(<ChartTooltip x={50} y={50} lines={["line1", "line2", "line3"]} />);
    expect(screen.getByText("line1")).toBeInTheDocument();
    expect(screen.getByText("line2")).toBeInTheDocument();
    expect(screen.getByText("line3")).toBeInTheDocument();
  });

  it("applies fail style to ✗ lines", () => {
    render(<ChartTooltip x={0} y={0} lines={["\u2717 vf_cw_weak"]} />);
    const el = screen.getByText(/vf_cw_weak/);
    expect(el).toHaveStyle({ color: "#ff6d00", fontWeight: 600 });
  });

  it("applies pass style to ✓ lines", () => {
    render(<ChartTooltip x={0} y={0} lines={["\u2713 vf_tsi"]} />);
    const el = screen.getByText(/vf_tsi/);
    expect(el).toHaveStyle({ color: "#26a69a", fontWeight: 600 });
  });

  it("positions based on x/y", () => {
    const { container } = render(
      <ChartTooltip x={100} y={200} lines={["test"]} containerWidth={1000} />
    );
    const tooltip = container.firstChild as HTMLElement;
    expect(tooltip.style.top).toBe("190px");
  });
});
