// Issue #540: the chat follows streamed text only while the user is at the
// bottom; a jump-to-latest button returns there.
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { onChatTextDelta } from "../events";
import { sendChatMessage } from "../api";
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api", async () => {
  const { anthropicProvider } = await import("../test/fixtures");
  const { createApiMock } = await import("../test/apiMock");
  return createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([anthropicProvider()])),
  });
});

vi.mock("../events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock
    .calls;
  return calls[calls.length - 1][0];
}

const CLIENT = 100;

/** jsdom has no layout: give the scroller fake, controllable metrics. */
function fakeMetrics(el: HTMLElement) {
  const m = { scrollHeight: 1000, scrollTop: 900 };
  Object.defineProperty(el, "scrollHeight", {
    get: () => m.scrollHeight,
    configurable: true,
  });
  Object.defineProperty(el, "clientHeight", {
    get: () => CLIENT,
    configurable: true,
  });
  Object.defineProperty(el, "scrollTop", {
    get: () => m.scrollTop,
    set: (v: number) => {
      m.scrollTop = Math.min(v, m.scrollHeight - CLIENT);
    },
    configurable: true,
  });
  return m;
}

async function setup() {
  renderChatPanel();
  await waitFor(() => expect(onChatTextDelta).toHaveBeenCalled());
  const scroller = await waitFor(() => {
    const el = document.querySelector(".overflow-y-auto");
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });
  const m = fakeMetrics(scroller);
  let text = "";
  const delta = (d: string) => {
    text += d;
    m.scrollHeight += 50; // streamed text grows the content
    act(() =>
      handlerOf<{ sessionId: string; delta: string }>(onChatTextDelta)({
        sessionId: "session-1",
        delta: d,
      }),
    );
  };
  const scrollTo = (top: number) => {
    m.scrollTop = top;
    fireEvent.scroll(scroller);
  };
  return { m, delta, scrollTo, text: () => text };
}

const jumpButton = () =>
  screen.queryByRole("button", { name: "Zur neuesten Nachricht" });

describe("ChatPanel scroll following (issue #540)", () => {
  beforeEach(() => vi.clearAllMocks());

  it("keeps following while at the bottom", async () => {
    const { m, delta } = await setup();
    delta("a");
    delta("b");
    expect(m.scrollHeight - m.scrollTop - CLIENT).toBeLessThanOrEqual(16);
    expect(jumpButton()).toBeNull();
  });

  it("leaves scrollTop alone once scrolled up, and shows the button", async () => {
    const { m, delta, scrollTo } = await setup();
    delta("a");
    scrollTo(300);
    expect(jumpButton()).not.toBeNull();
    delta("b");
    delta("c");
    expect(m.scrollTop).toBe(300);
    scrollTo(100);
    delta("d");
    expect(m.scrollTop).toBe(100);
  });

  it("resumes following when scrolled back to the bottom", async () => {
    const { m, delta, scrollTo } = await setup();
    delta("a");
    scrollTo(300);
    delta("b");
    scrollTo(m.scrollHeight - CLIENT - 10);
    expect(jumpButton()).toBeNull();
    delta("c");
    expect(m.scrollHeight - m.scrollTop - CLIENT).toBeLessThanOrEqual(16);
  });

  it("jump button scrolls down, hides itself and resumes following", async () => {
    const { m, delta, scrollTo } = await setup();
    delta("a");
    scrollTo(300);
    fireEvent.click(jumpButton()!);
    expect(m.scrollTop).toBe(m.scrollHeight - CLIENT);
    expect(jumpButton()).toBeNull();
    delta("b");
    expect(m.scrollHeight - m.scrollTop - CLIENT).toBeLessThanOrEqual(16);
  });

  it("sending a message while scrolled up jumps to the bottom", async () => {
    const { m, delta, scrollTo } = await setup();
    delta("a");
    scrollTo(300);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalled());
    expect(m.scrollHeight - m.scrollTop - CLIENT).toBeLessThanOrEqual(16);
    expect(jumpButton()).toBeNull();
  });
});
