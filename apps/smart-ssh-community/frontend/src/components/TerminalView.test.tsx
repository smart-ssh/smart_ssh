// Issue #565: the terminal opens exactly one new shell after any reconnect.
import { render } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";

vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
  },
);
const writeln = vi.fn();
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    loadAddon() {}
    open() {}
    onData() {
      return { dispose() {} };
    }
    writeln = writeln;
    write() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));
vi.mock("../events", () => ({
  base64ToBytes: () => new Uint8Array(),
  onTerminalOutput: () => Promise.resolve(() => {}),
}));
const openTerminal = vi.fn(() => Promise.resolve(null));
vi.mock("../api", () => ({
  commandErrorMessage: String,
  openTerminal: (...a: unknown[]) => openTerminal(...(a as [])),
  terminalInput: () => Promise.resolve(),
  terminalResize: () => Promise.resolve(),
}));
vi.mock("../startDirectory", () => ({ notifyMissingStartDirectory: () => {} }));

import { TerminalView, type TerminalLink } from "./TerminalView";

function run(links: TerminalLink[]) {
  const ui = (link: TerminalLink) => (
    <I18nextProvider i18n={testI18n}>
      <TerminalView sessionId="s1" link={link} />
    </I18nextProvider>
  );
  const view = render(ui(links[0]));
  for (const l of links.slice(1)) view.rerender(ui(l));
  view.unmount();
}

describe("TerminalView reconnect", () => {
  beforeEach(() => {
    writeln.mockClear();
    openTerminal.mockClear();
  });

  it("live → reconnecting → live starts one new shell", () => {
    run(["live", "reconnecting", "live"]);
    expect(openTerminal).toHaveBeenCalledTimes(2); // initial + new
    expect(writeln).toHaveBeenCalledTimes(2); // loss notice + separator
  });

  it("live → lost → reconnecting → live starts one new shell", () => {
    run(["live", "lost", "reconnecting", "live"]);
    expect(openTerminal).toHaveBeenCalledTimes(2);
  });

  it("live → live starts none", () => {
    run(["live", "live"]);
    expect(openTerminal).toHaveBeenCalledTimes(1);
    expect(writeln).not.toHaveBeenCalled();
  });
});
