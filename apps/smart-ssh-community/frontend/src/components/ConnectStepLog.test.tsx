// Issue #51: die zugeklappte Detailansicht des Schritt-Protokolls.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { ConnectStepRecord } from "../types";
import { ConnectStepLog } from "./ConnectStepLog";

const steps: ConnectStepRecord[] = [
  {
    hopIndex: 0,
    hop: "deploy@jump:22",
    step: { kind: "dnsResolution", host: "jump", port: 22, addresses: ["192.0.2.1"] },
    status: { state: "ok" },
    durationMs: 3,
  },
  {
    hopIndex: 1,
    hop: "root@target:22",
    step: { kind: "tunnelOpen", host: "target", port: 22 },
    status: { state: "ok" },
    durationMs: 7,
  },
  {
    hopIndex: 1,
    hop: "root@target:22",
    step: {
      kind: "authentication",
      method: "identityFile",
      remainingMethods: ["publickey"],
      partialSuccess: false,
    },
    status: { state: "failed", code: "SSH_AUTH_FAILED" },
    durationMs: 12,
  },
];

function renderLog() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ConnectStepLog steps={steps} />
    </I18nextProvider>,
  );
}

describe("ConnectStepLog (issue #51)", () => {
  it("is collapsed by default and lists steps per hop with the failing one marked", () => {
    renderLog();
    const details = screen.getByTestId("connect-step-log");
    expect(details).not.toHaveAttribute("open");
    expect(screen.getByText("Hop 1: deploy@jump:22")).toBeInTheDocument();
    expect(screen.getByText("Hop 2: root@target:22")).toBeInTheDocument();
    const failed = details.querySelectorAll("[data-failed='true']");
    expect(failed).toHaveLength(1);
    expect(failed[0].textContent).toContain("Anmeldung");
    expect(failed[0].textContent).toContain("SSH_AUTH_FAILED");
    expect(failed[0].textContent).toContain("12 ms");
  });

  it("copies the text version to the clipboard", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderLog();
    fireEvent.click(screen.getByText("Kopieren"));
    await waitFor(() => expect(screen.getByText("Kopiert")).toBeInTheDocument());
    const text = (writeText.mock.calls[0] as unknown as [string])[0];
    expect(text).toContain("Hop 2: root@target:22");
    expect(text).toContain("✗ Anmeldung — fehlgeschlagen (SSH_AUTH_FAILED)");
  });

  it("says so when copying fails", async () => {
    const writeText = vi.fn(() => Promise.reject(new Error("no clipboard")));
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderLog();
    fireEvent.click(screen.getByText("Kopieren"));
    await waitFor(() =>
      expect(
        screen.getByText("Kopieren in die Zwischenablage ist fehlgeschlagen."),
      ).toBeInTheDocument(),
    );
  });
});
