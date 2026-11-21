import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ICON_NAMES, Icon } from "./Icon";

describe("Icon", () => {
  it("covers the design set", () => {
    expect(ICON_NAMES.length).toBeGreaterThanOrEqual(54);
    for (const n of ["folder", "folder-open", "server", "local", "chev-r", "home", "globe"]) {
      expect(ICON_NAMES).toContain(n);
    }
  });

  it.each(ICON_NAMES)("renders %s decoratively", (name) => {
    const { container } = render(<Icon name={name} />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("viewBox")).toBe("0 0 24 24");
    expect(svg.getAttribute("stroke")).toBe("currentColor");
    expect(svg.getAttribute("fill")).toBe("none");
    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.getAttribute("role")).toBeNull();
    expect(svg.querySelector("path")!.getAttribute("d")).toBeTruthy();
  });

  it.each(ICON_NAMES)("renders %s with a label as img", (name) => {
    const { container } = render(<Icon name={name} label="Thing" size={20} />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("role")).toBe("img");
    expect(svg.getAttribute("aria-label")).toBe("Thing");
    expect(svg.getAttribute("aria-hidden")).toBeNull();
    expect(svg.getAttribute("stroke-width")).toBe("2");
  });

  it("uses stroke 1.5 at 16px", () => {
    const { container } = render(<Icon name="check" />);
    expect(container.querySelector("svg")!.getAttribute("stroke-width")).toBe("1.5");
  });
});
