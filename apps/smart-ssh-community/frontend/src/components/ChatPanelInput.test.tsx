// Spec 0055, Teil 1 ("Testbarkeit"): "Enter sendet; Shift+Enter fügt
// Zeilenumbruch; Feld wächst bis Max, dann intern scrollbar." Eigene Datei
// statt einer Erweiterung von `ChatPanel.test.tsx` — jene Datei testet
// bewusst nur `ChatItemView` isoliert (kein API-Mocking nötig), das
// Eingabefeld lebt aber in `ChatPanel` selbst, das für einen sinnvollen
// Render deutlich mehr Abhängigkeiten mocken muss.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { continueTruncatedResponse, sendChatMessage, stopAutoContinuation } from "../api";
import {
  onChatQueuedMessagesSent,
  onChatResponseCancelled,
  onChatResponseTruncated,
  onChatTextDelta,
} from "../events";
import { testI18n } from "../testI18n";
import { ChatPanel } from "./ChatPanel";

// jsdom implementiert `Element.scrollTo` nicht — `ChatPanel`s
// Auto-Scroll-Effekt (unabhängig von dieser Spec) ruft es bei jeder
// `items`-Änderung auf.
if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  acceptAndCreateRule: vi.fn(),
  cancelRunningCommand: vi.fn(),
  continueTruncatedResponse: vi.fn(() => Promise.resolve()),
  exportDocument: vi.fn(),
  getChatHistory: vi.fn(() => Promise.resolve([])),
  listAiProviders: vi.fn(() =>
    Promise.resolve([
      {
        id: "p1",
        providerType: "anthropic",
        displayName: "Claude",
        baseUrl: null,
        model: "claude",
        supportsNativeToolCalling: true,
        isActive: true,
        extraHeaders: [],
        attestationUrl: null,
      },
    ]),
  ),
  listPromptHistory: vi.fn(() => Promise.resolve([])),
  respondToAction: vi.fn(),
  sendChatMessage: vi.fn(() => Promise.resolve()),
  stopAutoContinuation: vi.fn(() => Promise.resolve()),
  suggestRulePatterns: vi.fn(),
  takeChatContentIntoNote: vi.fn(),
}));

vi.mock("../events", () => ({
  onAiBudgetWaiting: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationLimitReached: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatDocumentGenerated: vi.fn(() => Promise.resolve(() => {})),
  onChatError: vi.fn(() => Promise.resolve(() => {})),
  onChatQueuedMessagesSent: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseCancelled: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatTextDelta: vi.fn(() => Promise.resolve(() => {})),
  onRiskAssessmentUpdated: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

function renderChatPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
}

describe("ChatPanel multiline input (Spec 0055, Teil 1)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders a textarea (not a single-line input) for the composer", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");
    expect(field.tagName).toBe("TEXTAREA");
  });

  it("Enter sends the message and clears the draft", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");

    fireEvent.change(field, { target: { value: "hallo" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(sendChatMessage).toHaveBeenCalledWith("session-1", "hallo"));
    expect(field).toHaveValue("");
  });

  it("Shift+Enter inserts a newline instead of sending", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");

    fireEvent.change(field, { target: { value: "erste Zeile" } });
    fireEvent.keyDown(field, { key: "Enter", shiftKey: true });

    // jsdom führt den nativen Zeilenumbruch-Einfügevorgang eines Textareas
    // bei `keyDown` nicht selbst aus (das ist Browser-Rendering-Verhalten,
    // kein vom Test simulierbares DOM-Ereignis) — die eigentlich prüfbare
    // Invariante ist: kein `preventDefault`-Sende-Pfad wurde ausgelöst,
    // `sendChatMessage` blieb unaufgerufen und der Inhalt unverändert.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sendChatMessage).not.toHaveBeenCalled();
    expect(field).toHaveValue("erste Zeile");
  });

  it("does not send an empty/whitespace-only draft on Enter", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");

    fireEvent.change(field, { target: { value: "   " } });
    fireEvent.keyDown(field, { key: "Enter" });

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sendChatMessage).not.toHaveBeenCalled();
  });

  it("grows the textarea's inline height as content grows (auto-grow effect)", async () => {
    renderChatPanel();
    const field = (await screen.findByPlaceholderText(
      "Frage stellen oder Kommando beschreiben …",
    )) as HTMLTextAreaElement;

    // jsdom liefert für `scrollHeight` immer 0 (kein echtes Layout) — die
    // hier prüfbare Invariante ist, dass der Auto-Grow-Effekt bei jeder
    // Draft-Änderung tatsächlich läuft (setzt `style.height` neu, statt
    // die feste Zeilenhöhe unangetastet zu lassen), nicht der konkrete
    // Pixelwert.
    fireEvent.change(field, { target: { value: "a\nb\nc" } });
    await waitFor(() => expect(field.style.height).not.toBe(""));
  });
});

describe("ChatPanel stop (Spec 0066, §1)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows a Stopp button while a normal (non-automatic) response runs and calls stopAutoContinuation", async () => {
    let resolveSend: () => void = () => {};
    vi.mocked(sendChatMessage).mockImplementationOnce(
      () => new Promise<void>((resolve) => (resolveSend = resolve)),
    );
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");

    fireEvent.change(field, { target: { value: "hallo" } });
    fireEvent.keyDown(field, { key: "Enter" });

    const stop = await screen.findByRole("button", { name: "Stopp" });
    fireEvent.click(stop);
    expect(stopAutoContinuation).toHaveBeenCalledWith("session-1");
    resolveSend();
  });

  it("renders a cancelled notice when the backend reports chat-response-cancelled", async () => {
    let handler: ((event: { sessionId: string }) => void) | null = null;
    vi.mocked(onChatResponseCancelled).mockImplementation((h) => {
      handler = h;
      return Promise.resolve(() => {});
    });
    renderChatPanel();
    await waitFor(() => expect(handler).not.toBeNull());

    handler!({ sessionId: "session-1" });

    expect(await screen.findByText("⏹ Antwort abgebrochen.")).toBeInTheDocument();
  });
});

describe("ChatPanel queued messages (Spec 0066, §2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps the input enabled while a response runs and queues a second message", async () => {
    let resolveFirst: () => void = () => {};
    vi.mocked(sendChatMessage).mockImplementationOnce(
      () => new Promise<void>((resolve) => (resolveFirst = resolve)),
    );
    let queuedHandler: ((event: { sessionId: string }) => void) | null = null;
    vi.mocked(onChatQueuedMessagesSent).mockImplementation((h) => {
      queuedHandler = h;
      return Promise.resolve(() => {});
    });
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");

    fireEvent.change(field, { target: { value: "erste" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await screen.findByRole("button", { name: "Stopp" });

    expect(field).not.toBeDisabled();
    fireEvent.change(field, { target: { value: "Korrektur" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(sendChatMessage).toHaveBeenCalledWith("session-1", "Korrektur"));
    expect(field).toHaveValue("");
    expect(screen.getByText("⏳ Wird mit der nächsten Anfrage gesendet")).toBeInTheDocument();

    queuedHandler!({ sessionId: "session-1" });
    await waitFor(() =>
      expect(screen.queryByText("⏳ Wird mit der nächsten Anfrage gesendet")).not.toBeInTheDocument(),
    );
    resolveFirst();
  });
});

describe("ChatPanel send state edge cases (Spec 0066, review)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("clears a queued label once no send is pending anymore, even without the backend event", async () => {
    let resolveFirst: () => void = () => {};
    vi.mocked(sendChatMessage).mockImplementationOnce(
      () => new Promise<void>((resolve) => (resolveFirst = resolve)),
    );
    renderChatPanel();
    const field = await screen.findByPlaceholderText("Frage stellen oder Kommando beschreiben …");
    fireEvent.change(field, { target: { value: "erste" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await screen.findByRole("button", { name: "Stopp" });
    fireEvent.change(field, { target: { value: "zweite" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await screen.findByText("⏳ Wird mit der nächsten Anfrage gesendet");

    resolveFirst();

    await waitFor(() =>
      expect(screen.queryByText("⏳ Wird mit der nächsten Anfrage gesendet")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("button", { name: "Stopp" })).not.toBeInTheDocument();
  });

  it("shows the Stopp button while a 'Weiter' continuation runs", async () => {
    let deltaHandler: ((event: { sessionId: string; delta: string }) => void) | null = null;
    let truncatedHandler: ((event: { sessionId: string }) => void) | null = null;
    vi.mocked(onChatTextDelta).mockImplementation((h) => {
      deltaHandler = h as typeof deltaHandler;
      return Promise.resolve(() => {});
    });
    vi.mocked(onChatResponseTruncated).mockImplementation((h) => {
      truncatedHandler = h;
      return Promise.resolve(() => {});
    });
    let resolveContinue: () => void = () => {};
    vi.mocked(continueTruncatedResponse).mockImplementationOnce(
      () => new Promise<void>((resolve) => (resolveContinue = resolve)),
    );
    renderChatPanel();
    await waitFor(() => expect(deltaHandler).not.toBeNull());
    deltaHandler!({ sessionId: "session-1", delta: "Teilantwort" });
    truncatedHandler!({ sessionId: "session-1" });

    fireEvent.click(await screen.findByRole("button", { name: "Weiter" }));

    expect(await screen.findByRole("button", { name: "Stopp" })).toBeInTheDocument();
    resolveContinue();
  });
});
