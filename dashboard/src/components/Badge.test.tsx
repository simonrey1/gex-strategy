import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { Badge } from "./Badge";

describe("Badge", () => {
  it("renders the label text", () => {
    render(<Badge label="PW" variant="pw" />);
    expect(screen.getByText("PW")).toBeInTheDocument();
  });

  it("applies the correct variant class", () => {
    const { container } = render(<Badge label="EXIT" variant="exit" />);
    const el = container.querySelector(".badge");
    expect(el).toHaveClass("badge-exit");
  });

  it.each(["pw", "cw", "vf", "entry", "exit"] as const)(
    "renders variant %s",
    (variant) => {
      const { container } = render(<Badge label={variant.toUpperCase()} variant={variant} />);
      expect(container.querySelector(`.badge-${variant}`)).toBeInTheDocument();
    },
  );
});
