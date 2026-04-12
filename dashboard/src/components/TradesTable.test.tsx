import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { TradesTable } from "./TradesTable";
import type { Column } from "../types";

interface TestRow {
  name: string;
  value: number;
}

const columns: Column<TestRow>[] = [
  { key: "name", header: "Name", render: (r) => r.name },
  { key: "value", header: "Value", render: (r) => r.value, className: "mono" },
];

const rows: TestRow[] = [
  { name: "Alpha", value: 100 },
  { name: "Beta", value: -50 },
];

describe("TradesTable", () => {
  it("renders column headers", () => {
    render(<TradesTable columns={columns} rows={rows} />);
    expect(screen.getByText("Name")).toBeInTheDocument();
    expect(screen.getByText("Value")).toBeInTheDocument();
  });

  it("renders row data", () => {
    render(<TradesTable columns={columns} rows={rows} />);
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("100")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });

  it("renders empty state when no rows", () => {
    render(<TradesTable columns={columns} rows={[]} />);
    expect(screen.getByText("No trades yet")).toBeInTheDocument();
  });

  it("calls onRowClick when a row is clicked", () => {
    const onClick = vi.fn();
    render(<TradesTable columns={columns} rows={rows} onRowClick={onClick} />);
    fireEvent.click(screen.getByText("Alpha"));
    expect(onClick).toHaveBeenCalledWith(0);
  });

  it("renders expanded content when row is expanded", () => {
    render(
      <TradesTable
        columns={columns}
        rows={rows}
        expandedRow={0}
        renderExpanded={(row) => <div>Expanded: {row.name}</div>}
      />,
    );
    expect(screen.getByText("Expanded: Alpha")).toBeInTheDocument();
  });

  it("applies custom rowClassName", () => {
    const { container } = render(
      <TradesTable
        columns={columns}
        rows={rows}
        rowClassName={(_, i) => (i === 0 ? "win" : "loss")}
      />,
    );
    const tradeRows = container.querySelectorAll(".trade-row-static");
    expect(tradeRows[0]).toHaveClass("win");
    expect(tradeRows[1]).toHaveClass("loss");
  });

  it("applies column className to td", () => {
    const { container } = render(<TradesTable columns={columns} rows={rows} />);
    const monoCells = container.querySelectorAll("td.mono");
    expect(monoCells.length).toBe(2);
  });
});
