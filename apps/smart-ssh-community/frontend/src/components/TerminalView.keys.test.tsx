// Issue #678: the touch key row and sticky modifiers send the right bytes.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { setWide } from "../test/matchMedia";

vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
  },
);
let onDataCb: (d: string) => void = () => {};
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    modes = { applicationCursorKeysMode: false };
    loadAddon() {}
    open() {}
    focus() {}
    onData(cb: (d: string) => void) {
      onDataCb = cb;
      return { dispose() {} };
    }
    writeln() {}
    write() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));
vi.mock("../events", () => ({
  base64ToBytes: () => new Uint8Array(),
  onTerminalOutput: () => Promise.resolve(() => {}),
}));
const terminalInput = vi.fn((..._a: unknown[]) => Promise.resolve());
vi.mock("../api", () => ({
  commandErrorMessage: String,
  openTerminal: () => Promise.resolve(null),
  terminalInput: (...a: unknown[]) => terminalInput(...a),
  terminalResize: () => Promise.resolve(),
}));
vi.mock("../startDirectory", () => ({ notifyMissingStartDirectory: () => {} }));

import { TerminalView } from "./TerminalView";

const sent = () => terminalInput.mock.calls.map((c) => new TextDecoder().decode(c[1] as Uint8Array));
const show = () =>
  render(
    <I18nextProvider i18n={testI18n}>
      <TerminalView sessionId="s1" />
    </I18nextProvider>,
  );

function coarse(on: boolean) {
  const orig = window.matchMedia;
  window.matchMedia = ((q: string) => ({
    ...orig(q),
    matches: on && q.includes("pointer: coarse"),
    addEventListener() {},
    removeEventListener() {},
  })) as typeof window.matchMedia;
  return () => (window.matchMedia = orig);
}

describe("TerminalView key row", () => {
  beforeEach(() => {
    terminalInput.mockClear();
    setWide();
  });

  it("is absent without a coarse pointer", () => {
    show();
    expect(screen.queryByTestId("terminal-key-row")).toBeNull();
  });

  it("sends the sequences of the row keys", () => {
    const restore = coarse(true);
    show();
    for (const name of ["Esc", "Tab", "Pfeil nach oben", "Pfeil nach unten", "Pfeil nach links", "Pfeil nach rechts", "Home", "End", "Bild aufwärts", "Bild abwärts", "|", "~", "/", "-"])
      fireEvent.click(screen.getByRole("button", { name }));
    expect(sent()).toEqual(["\x1b", "\t", "\x1b[A", "\x1b[B", "\x1b[D", "\x1b[C", "\x1b[H", "\x1b[F", "\x1b[5~", "\x1b[6~", "|", "~", "/", "-"]);
    restore();
  });

  it("sticky Ctrl turns the next soft-keyboard c into ETX, once", () => {
    const restore = coarse(true);
    show();
    fireEvent.click(screen.getByRole("button", { name: "Ctrl" }));
    expect(screen.getByRole("button", { name: "Ctrl" })).toHaveAttribute("aria-pressed", "true");
    act(() => onDataCb("c"));
    act(() => onDataCb("c"));
    expect(sent()).toEqual(["\x03", "c"]);
    expect(screen.getByRole("button", { name: "Ctrl" })).toHaveAttribute("aria-pressed", "false");
    restore();
  });

  it("sticky Alt turns b into ESC b; touch targets are 44px", () => {
    const restore = coarse(true);
    show();
    fireEvent.click(screen.getByRole("button", { name: "Alt" }));
    act(() => onDataCb("b"));
    expect(sent()).toEqual(["\x1bb"]);
    expect(screen.getByRole("button", { name: "Alt" }).className).toContain("min-h-11");
    restore();
  });
});
