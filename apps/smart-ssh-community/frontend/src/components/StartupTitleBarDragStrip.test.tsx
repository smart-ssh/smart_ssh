// Issue #163 / Spec 0014, Abschnitt 5: Auch auf den Startmasken vor der
// Entsperrung lässt sich das Fenster an der Oberkante ziehen — mit und ohne
// offenen Startdialog —, ohne dass die Maske dafür ein Kommando aufruft, das
// das Starttor abweist (Spec 0101, A16).
//
// jsdom rechnet kein Layout und keine Trefferprüfung; geprüft werden daher
// Stapelreihenfolge (z-Indizes), Positionierung (fest oben, Titelleistenhöhe,
// Freiraum der Fenster-Controls) und der freigehaltene obere Rand.
import fs from "node:fs";
import path from "node:path";
import { act, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { parseAllowedWhileLocked } from "../../../../../scripts/check-tauri-commands.mjs";
import { onStartupPrompt, onStartupUnlocked } from "../events";
import { testI18n } from "../testI18n";
import type { StartupPromptRequest, StartupStateDto } from "../types";
import { AppHeader } from "./AppHeader";
import { StartupGate } from "./StartupGate";

const invokeMock = vi.fn();

// Bewusst **nicht** `../api` attrappiert: Die echten API-Funktionen gehen
// über `invoke`, so sieht der Test jedes Kommando, das die Maske auslöst.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("../events", () => ({
  onStartupPrompt: vi.fn(() => Promise.resolve(() => {})),
  onStartupUnlocked: vi.fn(() => Promise.resolve(() => {})),
}));

// `../i18n` spricht beim Import die Tauri-Plugins `store`/`os` an.
vi.mock("../i18n", () => ({
  applyStoredLanguage: vi.fn(() => Promise.resolve()),
}));

const REPO_ROOT = path.resolve(__dirname, "../../../../..");
const ALLOWED_WHILE_LOCKED: string[] = parseAllowedWhileLocked(
  fs.readFileSync(path.join(REPO_ROOT, "crates/app-shell/src/startup_gate.rs"), "utf8"),
);

function state(overrides: Partial<StartupStateDto> = {}): StartupStateDto {
  return {
    screen: "unlock",
    mode: "password",
    offersStartOver: true,
    failedUnlockAttempts: 3,
    language: "de",
    ...overrides,
  };
}

/** Antwortet wie das Backend im gesperrten Zustand: Was nicht auf der
 * Positivliste steht, wird abgewiesen. */
function mockLockedBackend(platform: string) {
  invokeMock.mockImplementation((command: string) => {
    if (!ALLOWED_WHILE_LOCKED.includes(command)) {
      return Promise.reject({ message: "locked", code: "APP_LOCKED" });
    }
    if (command === "get_platform") return Promise.resolve(platform);
    if (command === "get_startup_state") return Promise.resolve(state());
    return Promise.resolve(null);
  });
}

function renderGate(initial: StartupStateDto | null, children: React.ReactNode = <div>APP</div>) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <StartupGate initialState={initial}>{children}</StartupGate>
    </I18nextProvider>,
  );
}

function deliverPrompt(request: StartupPromptRequest) {
  const handler = vi.mocked(onStartupPrompt).mock.calls.at(-1)?.[0];
  if (!handler) throw new Error("no listener for startup:prompt");
  act(() => handler(request));
}

function strip(): HTMLElement | null {
  return screen.queryByTestId("startup-titlebar-drag-strip");
}

function zIndexOf(el: Element): number {
  const inline = (el as HTMLElement).style.zIndex;
  if (inline) return Number(inline);
  const match = /(?:^|\s)z-(\d+)(?:\s|$)/.exec(el.getAttribute("class") ?? "");
  return match ? Number(match[1]) : 0;
}

/** Höchster z-Index eines Elements und seiner Vorfahren. */
function stackRankOf(el: Element): number {
  let rank = 0;
  for (let node: Element | null = el; node; node = node.parentElement) {
    rank = Math.max(rank, zIndexOf(node));
  }
  return rank;
}

function invokedCommands(): string[] {
  return invokeMock.mock.calls.map((call) => call[0] as string);
}

/** Jedes Eingabeelement liegt außerhalb jeder Drag-Region und unter
 * keiner Ebene, die über der Leiste läge. */
function expectInteractiveElementsOutsideDragRegion() {
  const controls = document.querySelectorAll("input, button, textarea, select, a[href]");
  expect(controls.length).toBeGreaterThan(0);
  for (const el of controls) {
    expect(el.closest("[data-tauri-drag-region]")).toBeNull();
    expect(strip()!.contains(el)).toBe(false);
  }
}

const question: StartupPromptRequest = {
  kind: "newMasterPassword",
  title: "Neues Master-Passwort",
  message: "Bitte ein Passwort wählen.",
};

describe("startup screens: title bar drag strip (issue #163)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invokeMock.mockReset();
    vi.mocked(onStartupPrompt).mockResolvedValue(() => {});
    vi.mocked(onStartupUnlocked).mockResolvedValue(() => {});
  });

  it("marks a strip at the top of the unlock screen as drag region, not covered by the card", async () => {
    mockLockedBackend("windows");
    const { container } = renderGate(state());
    await waitFor(() => expect(invokedCommands()).toContain("get_platform"));

    const layer = strip();
    expect(layer).not.toBeNull();
    expect(layer).toHaveAttribute("data-tauri-drag-region");
    expect(layer).toHaveAttribute("aria-hidden", "true");
    expect(layer).toBeEmptyDOMElement();
    // Fest an der Fensteroberkante, so hoch wie die Titelleiste (`h-9`).
    expect(layer).toHaveClass("fixed", "top-0", "h-9");

    // Die Karte liegt nicht in der Leiste, hat keinen Stapelrang darüber,
    // und die Maske hält oben Titelleistenhöhe + Rand frei (`pt-15` =
    // `h-9` + `p-6`), sodass die zentrierte Karte nie darunter rückt.
    const card = screen.getByRole("heading", { name: "Smart SSH" }).parentElement!;
    expect(layer!.contains(card)).toBe(false);
    expect(stackRankOf(card)).toBeLessThan(zIndexOf(layer!));
    const gateRoot = container.firstElementChild!;
    expect(gateRoot.contains(card)).toBe(true);
    expect(gateRoot).toHaveClass("pt-15");
    expect(gateRoot).not.toHaveClass("p-6");

    expectInteractiveElementsOutsideDragRegion();
    // Passwortfeld, Entsperren, Beenden, Neu anfangen — alle vorhanden.
    expect(screen.getByLabelText(/Master-Passwort/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Entsperren" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Beenden" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Neu anfangen" })).toBeTruthy();
  });

  it.each([
    ["state unknown", null],
    ["unusable wrapping", state({ screen: "unusableWrapping" })],
    ["unreachable wrapping", state({ screen: "unreachableWrapping" })],
    ["continuing in the window", state({ screen: "setUpMasterPassword", mode: "keychain" })],
  ] as const)("shows the strip on the %s screen", async (_name, initial) => {
    mockLockedBackend("linux");
    renderGate(initial);
    await waitFor(() => expect(invokedCommands()).toContain("get_platform"));

    expect(strip()).toHaveAttribute("data-tauri-drag-region");
    expectInteractiveElementsOutsideDragRegion();
  });

  it("stays above the backdrop of an open startup prompt without covering the dialog", async () => {
    mockLockedBackend("windows");
    renderGate(state());
    await waitFor(() => expect(vi.mocked(onStartupPrompt)).toHaveBeenCalled());
    deliverPrompt(question);

    const backdrop = document.querySelector<HTMLElement>("[data-modal-backdrop]");
    expect(backdrop).not.toBeNull();
    const layer = strip()!;
    expect(layer).toHaveAttribute("data-tauri-drag-region");
    // Stapelreihenfolge: Leiste über dem Backdrop (`z-50`) …
    expect(zIndexOf(backdrop!)).toBe(50);
    expect(zIndexOf(layer)).toBeGreaterThan(zIndexOf(backdrop!));
    // … und unter den Fenster-Controls des Plugins (z-Index 100).
    expect(zIndexOf(layer)).toBeLessThan(100);
    // Kein Kind des Backdrops und nicht durchlässig.
    expect(backdrop!.contains(layer)).toBe(false);
    expect(layer).not.toHaveClass("pointer-events-none");
    // Der Dialog hält oben die Titelleistenhöhe frei (`pt-13` = `h-9` +
    // `p-4`), seine Knöpfe und Eingaben rücken nie unter die Leiste.
    expect(backdrop).toHaveClass("pt-13");
    expect(backdrop).not.toHaveClass("p-4");
    expectInteractiveElementsOutsideDragRegion();
    // Die Knöpfe und Eingaben des Dialogs sind da (und laut oben außerhalb
    // der Drag-Region).
    expect(screen.getByRole("button", { name: "Ja, fortfahren" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Abbrechen" })).toBeTruthy();
    expect(backdrop!.querySelectorAll("input").length).toBeGreaterThan(0);
  });

  it("leaves the macOS traffic lights free on the left", async () => {
    mockLockedBackend("macos");
    renderGate(state());

    await waitFor(() =>
      expect(strip()!.style.left).toContain("--tauri-plugin-decoration-left-clearance"),
    );
    expect(strip()!.style.left).toContain("78px");
    expect(strip()!.style.right).toBe("0px");
  });

  it.each(["windows", "linux"])("leaves the %s window controls free on the right", async (os) => {
    mockLockedBackend(os);
    renderGate(state());

    await waitFor(() =>
      expect(strip()!.style.right).toContain("--tauri-plugin-decoration-right-clearance"),
    );
    expect(strip()!.style.right).toContain("140px");
    expect(strip()!.style.left).toBe("0px");
  });

  it("uses the same control clearance as the unlocked header", async () => {
    mockLockedBackend("macos");
    renderGate(state());
    await waitFor(() => expect(strip()!.style.left).toContain("left-clearance"));
    const lockedLeft = strip()!.style.left;

    invokeMock.mockImplementation((command: string) => {
      if (command === "get_platform") return Promise.resolve("macos");
      if (command === "create_overlay_titlebar") return Promise.resolve("custom");
      return Promise.reject(new Error("not mocked"));
    });
    const { container } = render(
      <I18nextProvider i18n={testI18n}>
        <AppHeader />
      </I18nextProvider>,
    );
    const header = container.querySelector("header")!;
    await waitFor(() => expect(header.style.paddingLeft).toContain("left-clearance"));
    expect(lockedLeft).toBe(header.style.paddingLeft);
  });

  it("invokes only commands from the locked allow list", async () => {
    mockLockedBackend("macos");
    renderGate(null);
    await waitFor(() => expect(invokedCommands()).toContain("get_startup_state"));
    await waitFor(() => expect(invokedCommands()).toContain("get_platform"));
    deliverPrompt(question);
    // Kurz Zeit für nachlaufende Effekte.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(ALLOWED_WHILE_LOCKED).toContain("get_platform");
    expect(ALLOWED_WHILE_LOCKED).not.toContain("create_overlay_titlebar");
    expect(ALLOWED_WHILE_LOCKED).not.toContain("get_app_info");
    const invoked = invokedCommands();
    expect(invoked).not.toContain("create_overlay_titlebar");
    expect(invoked).not.toContain("get_app_info");
    for (const command of invoked) {
      expect(ALLOWED_WHILE_LOCKED).toContain(command);
    }
  });

  it("shows only the header's drag handling once unlocked, no second strip", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_platform") return Promise.resolve("windows");
      if (command === "create_overlay_titlebar") return Promise.resolve("custom");
      return Promise.reject(new Error("not mocked"));
    });
    const { container } = renderGate(state({ screen: "unlocked" }), <AppHeader />);
    await waitFor(() => expect(invokedCommands()).toContain("create_overlay_titlebar"));

    expect(container.querySelector("header")).toHaveAttribute("data-tauri-drag-region");
    expect(strip()).toBeNull();
    expect(screen.queryByTestId("titlebar-drag-layer")).toBeNull();

    // Ein Hinweis, der die Entsperrung überdauert: genau eine Drag-Schicht
    // (die des Headers), keine zusätzliche Startleiste.
    await waitFor(() => expect(vi.mocked(onStartupPrompt)).toHaveBeenCalled());
    deliverPrompt({ ...question, kind: "notice" });
    await waitFor(() => expect(screen.queryByTestId("titlebar-drag-layer")).not.toBeNull());
    expect(strip()).toBeNull();
    expect(document.querySelectorAll("[data-testid$='drag-layer'], [data-testid$='drag-strip']")).toHaveLength(1);
  });
});
