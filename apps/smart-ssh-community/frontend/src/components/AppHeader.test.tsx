// Spec 0049, Fund 3/4: die Titelleiste rendert selbst keine eigenen
// Minimieren-/Maximieren-/Schließen-Buttons (Spec 0014, Abschnitt 2/4: das
// `tauri-plugin-decoration`-Plugin rendert und verdrahtet diese Controls
// selbst, sobald `create_overlay_titlebar` erfolgreich aktiviert — "wir
// bauen keine eigenen Nachbauten dieser Buttons"). Ein "Button-Präsenz"-Test
// im wörtlichen Sinn (wie die Spec ihn für Fund 3/4 vorschlägt) geht daher
// ins Leere; was diese Komponente tatsächlich selbst steuert und was hier
// getestet wird, ist die Drag-Region-Attributierung und das
// plattform-/aktivierungsabhängige Platz-Reservieren für die
// Plugin-Controls.
import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppHeader } from "./AppHeader";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function mockCommands(platform: string, decorationMode: string) {
  invokeMock.mockImplementation((command: string) => {
    if (command === "get_platform") return Promise.resolve(platform);
    if (command === "create_overlay_titlebar") return Promise.resolve(decorationMode);
    return Promise.reject(new Error(`unexpected invoke: ${command}`));
  });
}

describe("AppHeader drag region", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("marks the header and its noninteractive wrapper divs as a drag region", async () => {
    mockCommands("macos", "custom");
    const { container } = render(<AppHeader>tabs</AppHeader>);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const header = container.querySelector("header");
    expect(header).toHaveAttribute("data-tauri-drag-region");
    const draggableDivs = container.querySelectorAll("div[data-tauri-drag-region]");
    expect(draggableDivs.length).toBeGreaterThan(0);
  });
});

describe("AppHeader platform/decoration-mode layout (Spec 0049, Fund 3/4)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("reserves left padding for the macOS traffic lights while custom decoration is active", async () => {
    mockCommands("macos", "custom");
    const { container } = render(<AppHeader />);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const header = await waitFor(() => {
      const el = container.querySelector("header");
      expect(el).not.toBeNull();
      expect(el?.style.paddingLeft).not.toBe("16px");
      return el as HTMLElement;
    });
    expect(header.style.paddingLeft).toContain("78px");
    expect(header.style.paddingRight).toBe("16px");
  });

  it("reserves right padding for the Windows plugin controls while custom decoration is active", async () => {
    mockCommands("windows", "custom");
    const { container } = render(<AppHeader />);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const header = await waitFor(() => {
      const el = container.querySelector("header");
      expect(el).not.toBeNull();
      expect(el?.style.paddingRight).not.toBe("16px");
      return el as HTMLElement;
    });
    expect(header.style.paddingLeft).toBe("16px");
    expect(header.style.paddingRight).toContain("140px");
  });

  /** Der eigentliche Fund-3/4-Regressionstest: schlägt die Aktivierung der
   * Overlay-Titelleiste fehl (Backend fällt auf die native Titelleiste
   * zurück, s. `commands::create_overlay_titlebar`s `"native"`-Rückgabe),
   * darf dieser Header keinen Platz mehr für Plugin-Controls reservieren,
   * die es dann gar nicht gibt — sonst bliebe eine sichtbare leere Lücke.
   * Vor dieser Änderung ignorierte `AppHeader` den Rückgabewert von
   * `create_overlay_titlebar` komplett und reservierte immer Platz. */
  it("reserves no extra padding once decoration falls back to the native titlebar", async () => {
    mockCommands("windows", "native");
    const { container } = render(<AppHeader />);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const header = await waitFor(() => {
      const el = container.querySelector("header");
      expect(el).not.toBeNull();
      expect(el?.style.paddingRight).toBe("16px");
      return el as HTMLElement;
    });
    expect(header.style.paddingLeft).toBe("16px");
  });

  it("falls back to no extra padding if create_overlay_titlebar itself rejects", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_platform") return Promise.resolve("windows");
      if (command === "create_overlay_titlebar") return Promise.reject(new Error("boom"));
      return Promise.reject(new Error(`unexpected invoke: ${command}`));
    });
    const { container } = render(<AppHeader />);

    const header = await waitFor(() => {
      const el = container.querySelector("header");
      expect(el).not.toBeNull();
      expect(el?.style.paddingRight).toBe("16px");
      return el as HTMLElement;
    });
    expect(header.style.paddingLeft).toBe("16px");
  });
});
