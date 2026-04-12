import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { StatusCards } from "./StatusCards";
import type { CardDef } from "./StatusCards";

const cards: CardDef[] = [
  { label: "Net P&L", value: "+$500", colorClass: "positive" },
  { label: "Trades", value: "12", detail: "8W / 4L" },
  { label: "Loss", value: "-$200", colorClass: "negative" },
];

describe("StatusCards", () => {
  it("renders all cards", () => {
    render(<StatusCards cards={cards} />);
    expect(screen.getByText("Net P&L")).toBeInTheDocument();
    expect(screen.getByText("+$500")).toBeInTheDocument();
    expect(screen.getByText("Trades")).toBeInTheDocument();
    expect(screen.getByText("12")).toBeInTheDocument();
  });

  it("renders detail text when provided", () => {
    render(<StatusCards cards={cards} />);
    expect(screen.getByText("8W / 4L")).toBeInTheDocument();
  });

  it("applies color classes", () => {
    const { container } = render(<StatusCards cards={cards} />);
    expect(container.querySelector(".positive")).toBeInTheDocument();
    expect(container.querySelector(".negative")).toBeInTheDocument();
  });

  it("renders empty when no cards", () => {
    const { container } = render(<StatusCards cards={[]} />);
    expect(container.querySelector(".card")).not.toBeInTheDocument();
  });
});
