// Spec 0029, Abschnitt 4: strukturelle DOM-Prüfung, dass die Risiko-Badges
// im selben Zeilen-Container wie das Aktions-Label sitzen, statt als
// eigener Block über dem Kommando-Text.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { RiskAssessment } from "../types";
import { ChatItemView, type ChatItem } from "./ChatPanel";

type ActionItem = Extract<ChatItem, { type: "action" }>;

function buildActionItem(overrides: Partial<ActionItem> = {}): ChatItem {
  return {
    type: "action",
    id: "item-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: "uname -a" } },
    decision: { Confirm: { reason: "keine Regel gefunden", code: "FILTER_NO_RULE_MATCHED" } },
    responded: false,
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    riskSecondOpinionPending: false,
    startedAt: null,
    origin: { kind: "internal" },
    ...overrides,
  };
}

const riskyAssessment: RiskAssessment = {
  serverRisk: "yellow",
  serverRiskReason: "systemctl-Kommando",
  dataRisk: "none",
  dataRiskReason: null,
  aiReviewed: false,
};

const noRiskAssessment: RiskAssessment = {
  serverRisk: "none",
  serverRiskReason: null,
  dataRisk: "none",
  dataRiskReason: null,
  aiReviewed: false,
};

function renderItem(item: ChatItem) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatItemView
        item={item}
        onRespond={vi.fn()}
        onAcceptWithRule={vi.fn()}
        onExport={vi.fn()}
        serverId="server-1"
      />
    </I18nextProvider>,
  );
}

describe("risk badge positioning (Spec 0029)", () => {
  it("places the risk badge and decision badge in the same row container as the action label", () => {
    const { container } = renderItem(buildActionItem({ riskAssessment: riskyAssessment }));

    const label = screen.getByText("Kommando vorschlagen");
    const riskBadge = screen.getByText("Server");
    const decisionBadge = screen.getByText("Bestätigung nötig");
    const commandBlock = container.querySelector("code");

    const row = label.parentElement;
    expect(row).not.toBeNull();
    // Spec 0029, Abschnitt 4: "Badges befinden sich im selben
    // Zeilen-Container wie das Aktions-Label" — beide Badges müssen also
    // Nachfahren desselben Zeilen-Containers sein wie das Label selbst.
    expect(row?.contains(riskBadge)).toBe(true);
    expect(row?.contains(decisionBadge)).toBe(true);

    // Der Kommando-Textblock bleibt unverändert außerhalb dieser Zeile
    // (direkt darunter, nicht Teil davon).
    expect(commandBlock).not.toBeNull();
    expect(row?.contains(commandBlock)).toBe(false);
  });

  it("keeps the risk badge to the right of the label within the row (Server -> Confirm order)", () => {
    renderItem(buildActionItem({ riskAssessment: riskyAssessment }));

    const label = screen.getByText("Kommando vorschlagen");
    const riskBadge = screen.getByText("Server");
    const decisionBadge = screen.getByText("Bestätigung nötig");

    // DOCUMENT_POSITION_FOLLOWING (4): das erste Argument kommt im Dokument
    // vor dem zweiten.
    // eslint-disable-next-line no-bitwise
    expect(label.compareDocumentPosition(riskBadge) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // eslint-disable-next-line no-bitwise
    expect(riskBadge.compareDocumentPosition(decisionBadge) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders no risk-badge markup at all when both risk axes are none (no layout jump)", () => {
    renderItem(buildActionItem({ riskAssessment: noRiskAssessment }));

    expect(screen.queryByText("Server")).toBeNull();
    expect(screen.queryByText("Daten")).toBeNull();
    expect(screen.queryByText(/keine Garantie/)).toBeNull();
  });
});
