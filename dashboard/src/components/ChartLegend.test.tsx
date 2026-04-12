import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ChartLegend } from "./ChartLegend";
import type { WallKey } from "../types";

const noop = () => {};

describe("ChartLegend", () => {
  it("renders wall groups and non-grouped items", () => {
    render(
      <ChartLegend hidden={new Set<WallKey>()} onToggle={noop} onToggleGroup={noop} />
    );
    expect(screen.getByText("Put Walls")).toBeInTheDocument();
    expect(screen.getByText("Call Walls")).toBeInTheDocument();
    expect(screen.getByText("Sm PW")).toBeInTheDocument();
  });

  it("hides spike windows item when hasSpikeWindows is false", () => {
    render(
      <ChartLegend
        hidden={new Set<WallKey>()}
        onToggle={noop}
        onToggleGroup={noop}
        hasSpikeWindows={false}
        onToggleSpikeWindows={noop}
      />
    );
    expect(screen.queryByText("Spike Windows")).not.toBeInTheDocument();
  });

  it("shows spike windows item when hasSpikeWindows is true", () => {
    render(
      <ChartLegend
        hidden={new Set<WallKey>()}
        onToggle={noop}
        onToggleGroup={noop}
        hasSpikeWindows={true}
        onToggleSpikeWindows={noop}
        showSpikeWindows={true}
      />
    );
    expect(screen.getByText("Spike Windows")).toBeInTheDocument();
  });

  it("calls onToggleSpikeWindows on click", () => {
    const fn = vi.fn();
    render(
      <ChartLegend
        hidden={new Set<WallKey>()}
        onToggle={noop}
        onToggleGroup={noop}
        hasSpikeWindows={true}
        onToggleSpikeWindows={fn}
        showSpikeWindows={true}
      />
    );
    fireEvent.click(screen.getByText("Spike Windows"));
    expect(fn).toHaveBeenCalledOnce();
  });

  it("applies legend-off class when showSpikeWindows is false", () => {
    render(
      <ChartLegend
        hidden={new Set<WallKey>()}
        onToggle={noop}
        onToggleGroup={noop}
        hasSpikeWindows={true}
        onToggleSpikeWindows={noop}
        showSpikeWindows={false}
      />
    );
    const item = screen.getByText("Spike Windows").closest(".legend-item");
    expect(item?.className).toContain("legend-off");
  });
});
