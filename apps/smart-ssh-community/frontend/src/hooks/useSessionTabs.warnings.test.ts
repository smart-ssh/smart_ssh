// Issue #1173: the `connect-warnings` event shows the outdated-algorithms
// notice (and only when there are warnings).
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { onConnectWarnings } from "../api/events";
import { showToast } from "../buses/toastBus";
import i18next from "i18next";
import { LANGUAGE_REGISTRY } from "../lib/languages";
import { useSessionTabs } from "./useSessionTabs";

vi.mock("../api/api", async () => {
  return (await import("../test/apiMock")).createApiMock({
    listSessions: vi.fn(() => Promise.resolve([])),
  });
});
vi.mock("../buses/toastBus", () => ({ showToast: vi.fn() }));
vi.mock("../api/events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onConnectProgress: vi.fn(() => Promise.resolve(() => {})),
  onConnectWarnings: vi.fn(() => Promise.resolve(() => {})),
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onMcpActionTabRequested: vi.fn(() => Promise.resolve(() => {})),
  onChatSessionChanged: vi.fn(() => Promise.resolve(() => {})),
}));

beforeAll(async () => {
  // The toast uses the global i18next instance.
  const en = LANGUAGE_REGISTRY.find((l) => l.code === "en");
  await i18next.init({
    lng: "en",
    ns: ["common"],
    defaultNS: "common",
    resources: { en: { common: en?.resources ?? {} } },
    interpolation: { escapeValue: false },
  });
});

async function handler() {
  renderHook(() => useSessionTabs());
  await waitFor(() => expect(vi.mocked(onConnectWarnings)).toHaveBeenCalled());
  return vi.mocked(onConnectWarnings).mock.calls[0][0];
}

describe("connect-warnings wiring", () => {
  beforeEach(() => {
    vi.mocked(showToast).mockClear();
    vi.mocked(onConnectWarnings).mockClear();
  });

  it("shows the notice with the algorithms for a single-hop warning", async () => {
    const h = await handler();
    act(() =>
      h({
        sessionId: "s",
        hopCount: 1,
        warnings: [{ hop: "u@t:22", algorithms: ["ssh-rsa"] }],
      }),
    );
    const msg = vi.mocked(showToast).mock.calls[0][0].message;
    expect(msg).toContain("ssh-rsa");
    expect(msg).not.toContain("u@t:22");
  });

  it("names the hop when only the bastion of a chain is affected", async () => {
    const h = await handler();
    act(() =>
      h({
        sessionId: "s",
        hopCount: 2,
        warnings: [{ hop: "u@bastion:22", algorithms: ["aes128-cbc"] }],
      }),
    );
    expect(vi.mocked(showToast).mock.calls[0][0].message).toContain(
      "u@bastion:22",
    );
  });

  it("shows nothing without warnings", async () => {
    const h = await handler();
    act(() => h({ sessionId: "s", hopCount: 1, warnings: [] }));
    expect(showToast).not.toHaveBeenCalled();
  });
});
