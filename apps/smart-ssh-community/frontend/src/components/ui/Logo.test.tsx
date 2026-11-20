import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Logo } from "./Logo";

describe("Logo", () => {
  it("mark: image named Smart SSH, token colours only", () => {
    const { container } = render(<Logo />);
    expect(screen.getByRole("img", { name: "Smart SSH" })).toBeTruthy();
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("viewBox")).toBe("0 0 64 64");
    expect(svg.getAttribute("width")).toBe("20");
    expect(svg.querySelector("path")!.getAttribute("stroke")).toBe("currentColor");
    expect(svg.querySelectorAll("path")[1]!.getAttribute("class")).toBe("fill-accent");
    expect(container.innerHTML).not.toMatch(/#[0-9a-f]{3,6}/i);
  });

  it("mark: decorative next to visible text", () => {
    const { container } = render(<Logo decorative size={32} />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.getAttribute("role")).toBeNull();
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("lockup: glyph and wordmark, named Smart SSH once", () => {
    const { container } = render(<Logo variant="lockup" />);
    expect(screen.getAllByRole("img", { name: "Smart SSH" })).toHaveLength(1);
    expect(container.textContent).toBe("SMART SSH");
    expect(container.querySelector(".text-accent")).toBeNull();
    expect(container.querySelector("svg")!.getAttribute("aria-hidden")).toBe("true");
  });

  it("lockup: full wordmark is SMARTSSH with accent SSH", () => {
    const { container } = render(<Logo variant="lockup" wordmark="full" />);
    expect(container.textContent).toBe("SMARTSSH");
    expect(container.querySelector("span.text-accent")!.textContent).toBe("SSH");
  });

  it("lockup: decorative has no role", () => {
    render(<Logo variant="lockup" decorative />);
    expect(screen.queryByRole("img")).toBeNull();
  });
});
