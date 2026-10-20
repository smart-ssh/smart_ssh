// Issue #548: connecting / failed tab body.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { ConnectStepSnapshot } from "../types";
import type { SessionTab } from "../useSessionTabs";
import { ConnectingTabView } from "./ConnectingTabView";

const snap = (
  kind: ConnectStepSnapshot["kind"],
  status: ConnectStepSnapshot["status"],
  detail: ConnectStepSnapshot["detail"] = null,
): ConnectStepSnapshot => ({ kind, hopIndex: 0, hop: "u@h:22", status, errorCode: null, detail });

function tab(overrides: Partial<SessionTab>): SessionTab {
  return {
    sessionId: "connecting:a",
    serverId: "s",
    serverName: "prod-db",
    status: "disconnected",
    hasPendingAction: false,
    pendingActionId: null,
    mcp: null,
    phase: "connecting",
    attemptId: "a",
    progress: [],
    failure: null,
    ...overrides,
  };
}

function view(t: SessionTab, onRetry = vi.fn(), onClose = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <ConnectingTabView tab={t} isActiveTab onRetry={onRetry} onClose={onClose} />
    </I18nextProvider>,
  );
  return { onRetry, onClose };
}

describe("ConnectingTabView", () => {
  it("lists the steps with detail texts and offers Cancel while running", () => {
    const { onClose } = view(
      tab({
        progress: [
          snap("dnsResolution", "ok", { code: "contacting", target: "h:22" }),
          snap("hostKeyCheck", "running", { code: "waitingForDecision" }),
        ],
      }),
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("h:22");
    expect(items[1]).toHaveAttribute("data-status", "running");
    expect(items[1]).toHaveTextContent(testI18n.t("connectTab.detail.waitingForDecision"));
    fireEvent.click(screen.getByRole("button", { name: testI18n.t("connectTab.cancel") }));
    expect(onClose).toHaveBeenCalled();
  });

  it("announces only step completions, not detail changes of a running step", () => {
    const first = [snap("dnsResolution", "ok"), snap("tcpConnect", "running")];
    const { rerender } = render(
      <I18nextProvider i18n={testI18n}>
        <ConnectingTabView tab={tab({ progress: first })} isActiveTab onRetry={vi.fn()} onClose={vi.fn()} />
      </I18nextProvider>,
    );
    const live = document.querySelector("[aria-live='polite']")!;
    const before = live.textContent;
    expect(before).toContain(testI18n.t("connectLog.step.dnsResolution"));
    rerender(
      <I18nextProvider i18n={testI18n}>
        <ConnectingTabView
          tab={tab({ progress: [first[0], snap("tcpConnect", "running", { code: "contacting", target: "h:22" })] })}
          isActiveTab
          onRetry={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nextProvider>,
    );
    expect(live.textContent).toBe(before);
  });

  it("shows the failed step, error, Retry and Close, with focus on Retry", () => {
    const { onRetry, onClose } = view(
      tab({
        phase: "failed",
        attemptId: null,
        progress: [snap("tcpConnect", "ok"), snap("authentication", "failed")],
        failure: { message: "Anmeldung abgelehnt", steps: [] },
      }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Anmeldung abgelehnt");
    expect(document.querySelectorAll("[data-failed='true']")).toHaveLength(1);
    const retry = screen.getByRole("button", { name: testI18n.t("connectTab.retry") });
    expect(retry).toHaveFocus();
    fireEvent.click(retry);
    expect(onRetry).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: testI18n.t("connectTab.close") }));
    expect(onClose).toHaveBeenCalled();
  });
});
