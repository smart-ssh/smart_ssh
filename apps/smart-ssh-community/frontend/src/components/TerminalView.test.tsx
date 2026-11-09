// Issue #565: the terminal opens exactly one new shell after any reconnect.
import { render, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";

vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
  },
);
const writeln = vi.fn();
const terms = vi.hoisted(() => [] as { themeWrites: unknown[] }[]);
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    themeWrites: unknown[] = [];
    options: { theme: unknown };
    constructor(opts: { theme?: unknown }) {
      const writes = this.themeWrites;
      let current = opts.theme;
      this.options = {
        get theme() {
          return current;
        },
        set theme(v: unknown) {
          current = v;
          writes.push(v);
        },
      };
      terms.push(this);
    }
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
vi.mock("../api/events", () => ({
  base64ToBytes: () => new Uint8Array(),
  onTerminalOutput: () => Promise.resolve(() => {}),
}));
const openTerminal = vi.fn(() => Promise.resolve(null));
vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    openTerminal: (...a: unknown[]) => openTerminal(...(a as [])) as never,
    terminalInput: () => Promise.resolve(),
    terminalResize: () => Promise.resolve(),
  }),
);
vi.mock("../lib/startDirectory", () => ({
  notifyMissingStartDirectory: () => {},
}));

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

describe("TerminalView live theme switching", () => {
  const style = document.createElement("style");
  style.textContent = `
    :root { --term-bg: #111111; }
    [data-theme="light"] { --term-bg: #fafafa; }
    [data-theme="dark"] { --term-bg: #111111; }`;

  beforeEach(() => {
    document.head.appendChild(style);
    document.documentElement.setAttribute("data-theme", "dark");
    terms.length = 0;
  });
  afterEach(() => {
    style.remove();
    document.documentElement.removeAttribute("data-theme");
  });

  const bg = (t: { themeWrites: unknown[] }) =>
    (t as unknown as { options: { theme: { background: string } } }).options
      .theme.background;

  function mount() {
    return render(
      <I18nextProvider i18n={testI18n}>
        <TerminalView sessionId="s1" />
      </I18nextProvider>,
    );
  }

  it("dark → light → dark updates the xterm theme each time", async () => {
    const view = mount();
    const term = terms[0];
    expect(bg(term)).toBe("#111111");
    document.documentElement.setAttribute("data-theme", "light");
    await waitFor(() => expect(bg(term)).toBe("#fafafa"));
    document.documentElement.setAttribute("data-theme", "dark");
    await waitFor(() => expect(bg(term)).toBe("#111111"));
    expect(term.themeWrites.length).toBeGreaterThanOrEqual(2);
    view.unmount();
  });

  it("stops observing after unmount", async () => {
    const view = mount();
    const term = terms[0];
    view.unmount();
    document.documentElement.setAttribute("data-theme", "light");
    // Flush pending mutation records: a live observer would have fired by now.
    await new Promise<void>((r) => queueMicrotask(r));
    await Promise.resolve();
    expect(term.themeWrites).toHaveLength(0);
  });
});
