// Issue #160 / Spec 0014, Abschnitt 5: solange ein modaler Dialog offen
// ist, liegt eine transparente Drag-Schicht über der Titelleiste — über dem
// Backdrop des Dialogs —, damit das Fenster ziehbar bleibt und die
// interaktiven Header-Inhalte (Session-Tabs) blockiert sind.
//
// jsdom rechnet kein Layout und keine Trefferprüfung; geprüft wird daher
// die Stapelreihenfolge über die z-Indizes und die Abdeckung über die
// Positionierung (fest oben, Header-Höhe, Freiraum nur für die
// Fenster-Controls).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publishFeatureLocked } from "../extensions/featureLockedBus";
import { FeatureLockedDialog } from "../extensions/FeatureLockedDialog";
import { testI18n } from "../testI18n";
import type { HostKeyInfo } from "../types";
import { AppHeader } from "./AppHeader";
import { HostKeyDialog } from "./HostKeyDialog";
import { ModalBackdrop } from "./ModalBackdrop";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function mockCommands(platform: string, decorationMode: string) {
  invokeMock.mockImplementation((command: string) => {
    if (command === "get_platform") return Promise.resolve(platform);
    if (command === "create_overlay_titlebar") return Promise.resolve(decorationMode);
    if (command === "get_app_info") return Promise.reject(new Error("not mocked in this test"));
    return Promise.reject(new Error(`unexpected invoke: ${command}`));
  });
}

const hostKeyEvent: HostKeyInfo = {
  host: "example.org",
  port: 22,
  kind: "unknown",
  fingerprint: "SHA256:abc",
  expectedFingerprint: null,
};

/** z-Index eines Elements: Inline-Style vor Tailwind-Klasse `z-NN`; ohne
 * beides `0` (z-index `auto`, kein eigener Stapelrang). */
function zIndexOf(el: Element): number {
  const inline = (el as HTMLElement).style.zIndex;
  if (inline) return Number(inline);
  const match = /(?:^|\s)z-(\d+)(?:\s|$)/.exec(el.getAttribute("class") ?? "");
  return match ? Number(match[1]) : 0;
}

/** Rendert Header (mit einem interaktiven Tab) und optional einen Dialog —
 * wie in `App.tsx` als Geschwister unter derselben Wurzel. */
function renderApp(dialog: React.ReactNode) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <AppHeader>
        <button type="button">session tab</button>
      </AppHeader>
      {dialog}
    </I18nextProvider>,
  );
}

function backdrop(): HTMLElement {
  const el = document.querySelector<HTMLElement>("[data-modal-backdrop]");
  if (!el) throw new Error("no modal backdrop rendered");
  return el;
}

function dragLayer(): HTMLElement | null {
  return screen.queryByTestId("titlebar-drag-layer");
}

describe("title bar drag layer above modals (issue #160)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is absent without an open dialog, so the header behaves as before", async () => {
    mockCommands("windows", "custom");
    renderApp(null);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    expect(dragLayer()).toBeNull();
    expect(document.querySelector("[data-modal-backdrop]")).toBeNull();
  });

  it("covers the title bar above the host-key dialog's backdrop and blocks header content", async () => {
    mockCommands("windows", "custom");
    const { container } = renderApp(<HostKeyDialog event={hostKeyEvent} onDecision={vi.fn()} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const layer = dragLayer();
    expect(layer).not.toBeNull();
    // Ziehbar, ohne Inhalt: nur Fensterfunktion, keine App-Inhalte.
    expect(layer).toHaveAttribute("data-tauri-drag-region");
    expect(layer).toBeEmptyDOMElement();
    // Fest an der Fensteroberkante, so hoch wie der Header (`h-9`).
    const header = container.querySelector("header")!;
    expect(header).toHaveClass("h-9");
    expect(layer).toHaveClass("fixed", "top-0", "h-9");
    // Über dem Backdrop des Dialogs in der Stapelreihenfolge.
    expect(zIndexOf(backdrop())).toBe(50);
    expect(zIndexOf(layer!)).toBeGreaterThan(zIndexOf(backdrop()));
    // Kein Kind des Headers (dessen `backdrop-blur` wäre ein eigener
    // Stapelkontext, die Schicht käme dann nicht über den Backdrop) und
    // nicht durchlässig für Zeiger-Ereignisse.
    expect(header.contains(layer)).toBe(false);
    expect(layer).not.toHaveClass("pointer-events-none");
    // Der interaktive Header-Inhalt liegt horizontal unter der Schicht
    // (links ab 0, rechts nur der Controls-Freiraum frei) und hat selbst
    // keinen Stapelrang darüber -> per Zeiger nicht erreichbar.
    expect(layer!.style.left).toBe("0px");
    expect(layer!.style.right).toBe(header.style.paddingRight);
    const tab = screen.getByRole("button", { name: "session tab" });
    expect(header.contains(tab)).toBe(true);
    for (let el: Element | null = tab; el && el !== container; el = el.parentElement) {
      expect(zIndexOf(el)).toBeLessThan(zIndexOf(layer!));
    }
  });

  it("leaves the macOS traffic-light area free on the left", async () => {
    mockCommands("macos", "custom");
    const { container } = renderApp(<HostKeyDialog event={hostKeyEvent} onDecision={vi.fn()} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));

    const header = container.querySelector("header")!;
    const layer = dragLayer()!;
    expect(layer.style.left).toBe(header.style.paddingLeft);
    expect(layer.style.left).toContain("--tauri-plugin-decoration-left-clearance");
    expect(layer.style.right).toBe("0px");
  });

  it("works for a dialog from the extension registry and disappears once it closes", async () => {
    mockCommands("linux", "custom");
    renderApp(<FeatureLockedDialog />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));
    expect(dragLayer()).toBeNull();

    act(() => {
      publishFeatureLocked({ feature: "document_export", tier: "free" });
    });
    const layer = dragLayer();
    expect(layer).not.toBeNull();
    expect(zIndexOf(layer!)).toBeGreaterThan(zIndexOf(backdrop()));

    // Schließen-Button des Dialogs (der einzige Button außer dem Tab).
    const close = screen
      .getAllByRole("button")
      .find((b) => b.textContent !== "session tab")!;
    act(() => {
      close.click();
    });
    expect(document.querySelector("[data-modal-backdrop]")).toBeNull();
    expect(dragLayer()).toBeNull();
  });

  it("stays while one of two stacked dialogs is still open", async () => {
    mockCommands("windows", "custom");
    const app = (dialogs: number) => (
      <I18nextProvider i18n={testI18n}>
        <AppHeader>
          <button type="button">session tab</button>
        </AppHeader>
        {Array.from({ length: dialogs }, (_, i) => (
          <ModalBackdrop key={i} className="fixed inset-0 z-50 bg-black/70" />
        ))}
      </I18nextProvider>
    );
    const { rerender } = render(app(2));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("create_overlay_titlebar"));
    expect(dragLayer()).not.toBeNull();

    rerender(app(1));
    expect(dragLayer()).not.toBeNull();

    rerender(app(0));
    expect(dragLayer()).toBeNull();
  });
});

// Ein neuer Dialog mit eigenem `fixed inset-0`-Backdrop statt
// `ModalBackdrop` würde die Titelleiste wieder verdecken, ohne dass die
// Drag-Schicht davon erfährt.
describe("every full-window modal backdrop uses ModalBackdrop (issue #160)", () => {
  const SRC_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

  function sourceFiles(dir: string): string[] {
    return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) return sourceFiles(full);
      return /\.tsx$/.test(entry.name) && !/\.test\.tsx$/.test(entry.name) ? [full] : [];
    });
  }

  it("no source file renders a plain element with a fixed inset-0 backdrop", () => {
    const offenders = sourceFiles(SRC_DIR).flatMap((file) =>
      fs
        .readFileSync(file, "utf8")
        .split("\n")
        .map((line, i) => ({ line, i }))
        .filter(({ line }) => /<(?!ModalBackdrop\b)[A-Za-z]\w*[^>]*\bfixed\b[^>]*\binset-0\b/.test(line))
        .map(({ line, i }) => `${path.relative(SRC_DIR, file)}:${i + 1}: ${line.trim()}`),
    );
    expect(offenders, offenders.join("\n")).toEqual([]);
  });
});
