// Spec 0029, Abschnitt 4: strukturelle DOM-Prüfung, dass die Risiko-Badges
// im selben Zeilen-Container wie das Aktions-Label sitzen, statt als
// eigener Block über dem Kommando-Text.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  registerDocumentAction,
  resetRegistryForTests,
} from "../extensions/registry";
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
        sessionId="session-1"
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

// Spec 0045, Abschnitt 4/7: registrierte Dokument-Aktionen erscheinen neben
// dem bestehenden Markdown-Export-Button in der Dokument-Karte.
describe("registered document actions (Spec 0045)", () => {
  afterEach(() => {
    resetRegistryForTests();
  });

  function documentItem(): ChatItem {
    return {
      type: "document",
      id: "doc-1",
      title: "Ergebnis",
      contentMarkdown: "# Ergebnis\n\ninhalt",
    };
  }

  it("renders an active document action and invokes it with the document context on click", () => {
    const onInvoke = vi.fn();
    registerDocumentAction({ id: "save-word", label: "Als Word speichern", onInvoke });

    render(
      <I18nextProvider i18n={testI18n}>
        <ChatItemView
          item={documentItem()}
          onRespond={vi.fn()}
          onAcceptWithRule={vi.fn()}
          onExport={vi.fn()}
          serverId="server-1"
          sessionId="session-1"
        />
      </I18nextProvider>,
    );

    const button = screen.getByRole("button", { name: "Als Word speichern" });
    expect(button).not.toBeDisabled();
    // spec-reviewer-Fund: eine aktive Aktion darf kein Tooltip-Attribut
    // tragen — sonst könnte eine Implementierung `disabledReason` immer
    // als Tooltip setzen, unabhängig von `disabled`, ohne dass dieser Test
    // es merkt.
    expect(button).not.toHaveAttribute("title");

    fireEvent.click(button);

    expect(onInvoke).toHaveBeenCalledWith({
      contentMarkdown: "# Ergebnis\n\ninhalt",
      title: "Ergebnis",
    });
  });

  it("renders a disabled document action as visible, greyed out, with disabledReason as a tooltip", () => {
    const onInvoke = vi.fn();
    registerDocumentAction({
      id: "save-word",
      label: "Als Word speichern",
      onInvoke,
      disabled: true,
      disabledReason: "Erfordert einen kostenpflichtigen Plan",
    });

    render(
      <I18nextProvider i18n={testI18n}>
        <ChatItemView
          item={documentItem()}
          onRespond={vi.fn()}
          onAcceptWithRule={vi.fn()}
          onExport={vi.fn()}
          serverId="server-1"
          sessionId="session-1"
        />
      </I18nextProvider>,
    );

    const button = screen.getByRole("button", { name: "Als Word speichern" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Erfordert einen kostenpflichtigen Plan");

    fireEvent.click(button);
    expect(onInvoke).not.toHaveBeenCalled();
  });

  it("still renders the unchanged markdown export button alongside registered actions", () => {
    registerDocumentAction({ id: "save-word", label: "Als Word speichern", onInvoke: vi.fn() });

    render(
      <I18nextProvider i18n={testI18n}>
        <ChatItemView
          item={documentItem()}
          onRespond={vi.fn()}
          onAcceptWithRule={vi.fn()}
          onExport={vi.fn()}
          serverId="server-1"
          sessionId="session-1"
        />
      </I18nextProvider>,
    );

    expect(screen.getByRole("button", { name: "Als Markdown speichern" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Als Word speichern" })).toBeInTheDocument();
  });
});

// Spec 0055, Teil 2 ("Testbarkeit"): "Aktionen erscheinen nach der
// gewählten Regel (substanziell/Hover), bestehende Funktion (Export/Notiz)
// weiter erreichbar."
describe("assistant message actions (Spec 0055, Teil 2)", () => {
  function assistantItem(text: string): ChatItem {
    return { type: "assistant", id: "assistant-1", text };
  }

  function renderAssistantItem(text: string) {
    return render(
      <I18nextProvider i18n={testI18n}>
        <ChatItemView
          item={assistantItem(text)}
          onRespond={vi.fn()}
          onAcceptWithRule={vi.fn()}
          onExport={vi.fn()}
          serverId="server-1"
          sessionId="session-1"
        />
      </I18nextProvider>,
    );
  }

  // Spec-Reviewer-Fund (Spec 0055, Review des Gesamtpakets): eine frühere
  // Fassung blendete die Aktionen unter einer Mindestlänge komplett aus
  // dem DOM aus — das verletzte "Keine bestehende Funktion entfernen"
  // wörtlich (eine kurze, aber notizwürdige Antwort wäre für "In Notiz
  // übernehmen" gar nicht mehr erreichbar gewesen, auch nicht per
  // Tastatur). Die Aktionen sind jetzt für JEDE Antwort im DOM vorhanden,
  // unabhängig von der Länge — nur die Sichtbarkeit ist dezent (Hover/
  // Fokus statt permanent).
  it("keeps the action row reachable (in the DOM) even for a trivial reply", () => {
    renderAssistantItem("Ok, verstanden.");

    expect(screen.getByText("Export:")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Als Markdown/ })).toBeInTheDocument();
    expect(screen.getByTitle("In Notiz übernehmen")).toBeInTheDocument();
  });

  it("hides the action row visually by default (opacity, not display:none)", () => {
    const longReply =
      "Das Kommando hat drei Zeilen Ausgabe erzeugt, die wichtigste Information steht am Ende.";
    const { container } = renderAssistantItem(longReply);

    // Dezent statt permanent (Spec 0055, Teil 2): die Aktionsleiste ist im
    // DOM (per Tastatur erreichbar), aber visuell erst bei Hover/Fokus
    // eingeblendet — `opacity-0` + `group-hover:opacity-100` statt
    // `hidden`/`display: none`.
    const actionRow = screen.getByText("Export:").closest("div");
    expect(actionRow).toHaveClass("opacity-0");
    expect(actionRow).toHaveClass("group-hover:opacity-100");
    expect(actionRow).toHaveClass("focus-within:opacity-100");
    expect(container.querySelector(".group")).not.toBeNull();
  });
});
