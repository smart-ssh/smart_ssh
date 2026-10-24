// Spec 0055, Teil 1: Enter sendet; Shift+Enter fügt einen
// Zeilenumbruch ein; das Feld wächst bis zu einem Maximum, dann scrollt es intern. Eigene Datei
// statt einer Erweiterung von `ChatPanel.test.tsx` — jene Datei testet
// bewusst nur `ChatItemView` isoliert (kein API-Mocking nötig), das
// Eingabefeld lebt aber in `ChatPanel` selbst, das für einen sinnvollen
// Render deutlich mehr Abhängigkeiten mocken muss.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendChatMessage } from "../api";
import { testI18n } from "../testI18n";
import {
  cancelSpeechModelDownload,
  cancelVoiceRecording,
  getSelectedSpeechModel,
  getSpeechModelStatus,
  startSpeechModelDownload,
  startVoiceRecording,
  stopVoiceRecording,
} from "../voiceApi";
import { loadVoiceInputEnabled } from "../voiceInputSettings";
import { ChatPanel } from "./ChatPanel";

// jsdom implementiert `Element.scrollTo` nicht — `ChatPanel`s
// Auto-Scroll-Effekt (unabhängig von dieser Spec) ruft es bei jeder
// `items`-Änderung auf.
if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: (err: unknown) => (err as { code?: string } | null)?.code ?? null,
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
  respondToAction: vi.fn(() => Promise.resolve()),
  sendChatMessage: vi.fn(() => Promise.resolve()),
  stopAutoContinuation: vi.fn(() => Promise.resolve()),
  explainConfirmation: vi.fn(() => Promise.resolve({ segments: [], suggestions: [] })),
  takeChatContentIntoNote: vi.fn(),
}));

vi.mock("../events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onAiBudgetWaiting: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionOutput: vi.fn(() => Promise.resolve(() => {})),
  onChatActionWaitingInput: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationLimitReached: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatDocumentGenerated: vi.fn(() => Promise.resolve(() => {})),
  onChatQuestionAsked: vi.fn(() => Promise.resolve(() => {})),
  onChatError: vi.fn(() => Promise.resolve(() => {})),
  onChatAiRetrying: vi.fn(() => Promise.resolve(() => {})),
  onChatQueuedMessagesSent: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseCancelled: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseEmpty: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseRefused: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatContextUsage: () => Promise.resolve(() => {}),
  onChatContextCompacted: () => Promise.resolve(() => {}),
  onChatTextDelta: vi.fn(() => Promise.resolve(() => {})),
  onChatWebActivity: vi.fn(() => Promise.resolve(() => {})),
  onChatInjectionCheck: vi.fn(() => Promise.resolve(() => {})),
  onChatWebResearchUnavailable: vi.fn(() => Promise.resolve(() => {})),
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

const statusListeners: Array<(ev: unknown) => void> = [];
const partialListeners: Array<(ev: { text: string; revision: number }) => void> = [];
const emitPartial = (text: string, revision: number) =>
  act(() => partialListeners.forEach((h) => h({ text, revision })));

vi.mock("../voiceApi", async (importActual) => ({
  ...(await importActual<typeof import("../voiceApi")>()),
  getSelectedSpeechModel: vi.fn(() => Promise.resolve("base")),
  getSpeechModelStatus: vi.fn(() => Promise.resolve({ model: "base", state: "ready" })),
  startSpeechModelDownload: vi.fn(() => Promise.resolve()),
  cancelSpeechModelDownload: vi.fn(() => Promise.resolve()),
  startVoiceRecording: vi.fn(() => Promise.resolve()),
  stopVoiceRecording: vi.fn(() => Promise.resolve("hallo welt")),
  cancelVoiceRecording: vi.fn(() => Promise.resolve()),
  onSpeechModelStatus: vi.fn((h: (ev: unknown) => void) => {
    statusListeners.push(h);
    return Promise.resolve(() => {});
  }),
  onSpeechRecordingState: vi.fn(() => Promise.resolve(() => {})),
  onSpeechPartialText: vi.fn((h: (ev: { text: string; revision: number }) => void) => {
    partialListeners.push(h);
    return Promise.resolve(() => {});
  }),
}));

vi.mock("../voiceInputSettings", async (importActual) => ({
  ...(await importActual<typeof import("../voiceInputSettings")>()),
  loadVoiceInputEnabled: vi.fn(() => Promise.resolve(true)),
}));

const PLACEHOLDER = "Frage stellen oder Kommando beschreiben …";
const START = "Spracheingabe starten";

describe("ChatPanel voice input (Spec 0108, §5, issue #418)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    statusListeners.length = 0;
    partialListeners.length = 0;
    vi.mocked(stopVoiceRecording).mockResolvedValue("hallo welt");
    vi.mocked(getSpeechModelStatus).mockResolvedValue({ model: "base", state: "ready" } as never);
    vi.mocked(loadVoiceInputEnabled).mockResolvedValue(true);
  });

  it("inserts the transcript into the draft without sending", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(await screen.findByRole("button", { name: /Aufnahme beenden/ }));
    await waitFor(() => expect(field).toHaveValue("hallo welt"));
    expect(sendChatMessage).not.toHaveBeenCalled();
  });

  it("appends the transcript to an existing draft, separated by a space", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "ls und" } });
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(await screen.findByRole("button", { name: /Aufnahme beenden/ }));
    await waitFor(() => expect(field).toHaveValue("ls und hallo welt"));
    expect(sendChatMessage).not.toHaveBeenCalled();
  });

  it("Esc during recording discards it and leaves the draft unchanged", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "entwurf" } });
    fireEvent.click(await screen.findByRole("button", { name: START }));
    await screen.findByRole("button", { name: /Aufnahme beenden/ });
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(cancelVoiceRecording).toHaveBeenCalled());
    expect(stopVoiceRecording).not.toHaveBeenCalled();
    expect(await screen.findByRole("button", { name: START })).toBeInTheDocument();
    expect(field).toHaveValue("entwurf");
  });

  it("the shortcut starts and stops the recording while the panel has focus", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    await screen.findByRole("button", { name: START });
    field.focus();
    fireEvent.keyDown(field, { key: "M", code: "KeyM", ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(startVoiceRecording).toHaveBeenCalledTimes(1));
    await screen.findByRole("button", { name: /Aufnahme beenden/ });
    fireEvent.keyDown(field, { key: "M", code: "KeyM", ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(stopVoiceRecording).toHaveBeenCalledTimes(1));
  });

  it("without a model the first click opens the consent dialog; cancel makes no download", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({ model: "base", state: "not_downloaded" } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("142");
    expect(dialog).toHaveTextContent("huggingface.co");
    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(startSpeechModelDownload).not.toHaveBeenCalled();
    expect(cancelSpeechModelDownload).not.toHaveBeenCalled();
    expect(startVoiceRecording).not.toHaveBeenCalled();
    expect(getSelectedSpeechModel).toHaveBeenCalled();
  });

  it("confirming downloads the model and starts recording only once it is ready", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({ model: "base", state: "not_downloaded" } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(await screen.findByRole("button", { name: "Herunterladen" }));
    await waitFor(() => expect(startSpeechModelDownload).toHaveBeenCalledWith("base"));
    expect(startVoiceRecording).not.toHaveBeenCalled();
    await waitFor(() => expect(statusListeners.length).toBeGreaterThan(0));
    statusListeners.forEach((h) => h({ model: "base", state: "downloading", done: 5, total: 10 }));
    await screen.findByRole("progressbar");
    statusListeners.forEach((h) => h({ model: "base", state: "ready" }));
    await waitFor(() => expect(startVoiceRecording).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("cancelling during the download cancels it and does not record", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({ model: "base", state: "not_downloaded" } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(await screen.findByRole("button", { name: "Herunterladen" }));
    await waitFor(() => expect(startSpeechModelDownload).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(cancelSpeechModelDownload).toHaveBeenCalledWith("base"));
    expect(startVoiceRecording).not.toHaveBeenCalled();
  });

  it("shows the translated message for a backend error and returns to idle", async () => {
    vi.mocked(startVoiceRecording).mockRejectedValueOnce({ code: "speech_permission_denied", message: "x" });
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Mikrofon");
    expect(screen.getByRole("button", { name: START })).toBeInTheDocument();
  });

  it("hides the button when voice input is disabled", async () => {
    vi.mocked(loadVoiceInputEnabled).mockResolvedValue(false);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    await waitFor(() => expect(loadVoiceInputEnabled).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("button", { name: START })).toBeNull());
  });

  describe("live text (issue #595)", () => {
    const STOP = /Aufnahme beenden/;
    async function startWith(draft: string) {
      renderChatPanel();
      const field = await screen.findByPlaceholderText(PLACEHOLDER);
      if (draft) fireEvent.change(field, { target: { value: draft } });
      fireEvent.click(await screen.findByRole("button", { name: START }));
      await screen.findByRole("button", { name: STOP });
      await waitFor(() => expect(partialListeners.length).toBeGreaterThan(0));
      return field;
    }

    it("shows provisional text after the draft, marked and read-only, send disabled", async () => {
      const field = await startWith("ls und");
      await emitPartial("hallo", 1);
      expect(field).toHaveValue("ls und hallo");
      expect(field).toHaveAttribute("readonly");
      expect(field).toHaveAttribute("data-provisional", "true");
      expect(screen.getByRole("button", { name: "Senden" })).toBeDisabled();
      await emitPartial("hallo wel", 2);
      expect(field).toHaveValue("ls und hallo wel");
    });

    it("the final text replaces the provisional range; input is editable and focused afterwards", async () => {
      const field = await startWith("ls und");
      await emitPartial("hallo wel", 1);
      fireEvent.click(screen.getByRole("button", { name: STOP }));
      await waitFor(() => expect(field).toHaveValue("ls und hallo welt"));
      await waitFor(() => expect(field).not.toHaveAttribute("readonly"));
      expect(field).not.toHaveAttribute("data-provisional");
      await waitFor(() => expect(field).toHaveFocus());
      expect((field as HTMLTextAreaElement).selectionStart).toBe("ls und hallo welt".length);
      expect(sendChatMessage).not.toHaveBeenCalled();
    });

    it("an empty final result restores the draft", async () => {
      vi.mocked(stopVoiceRecording).mockResolvedValue("  ");
      const field = await startWith("entwurf");
      await emitPartial("noise", 1);
      fireEvent.click(screen.getByRole("button", { name: STOP }));
      await waitFor(() => expect(field).toHaveValue("entwurf"));
      await waitFor(() => expect(field).not.toHaveAttribute("readonly"));
    });

    it("a failed final pass keeps the provisional text and shows the error", async () => {
      vi.mocked(stopVoiceRecording).mockRejectedValue({ code: "speech_transcription_failed" });
      const field = await startWith("a");
      await emitPartial("b", 1);
      fireEvent.click(screen.getByRole("button", { name: STOP }));
      expect(await screen.findByRole("alert")).toBeInTheDocument();
      expect(field).toHaveValue("a b");
      await waitFor(() => expect(field).not.toHaveAttribute("readonly"));
    });

    it("Esc removes the provisional text and later events change nothing", async () => {
      const field = await startWith("entwurf");
      await emitPartial("hallo", 1);
      fireEvent.keyDown(window, { key: "Escape" });
      await waitFor(() => expect(field).toHaveValue("entwurf"));
      await emitPartial("spaet", 2);
      expect(field).toHaveValue("entwurf");
      expect(field).not.toHaveAttribute("readonly");
    });

    it("ignores stale revisions and events after Stop", async () => {
      const field = await startWith("");
      await emitPartial("zwei", 2);
      await emitPartial("eins", 1);
      expect(field).toHaveValue("zwei");
      fireEvent.click(screen.getByRole("button", { name: STOP }));
      await waitFor(() => expect(field).toHaveValue("hallo welt"));
      await emitPartial("spaet", 3);
      expect(field).toHaveValue("hallo welt");
    });

    it("leaving the chat while recording cancels the recording", async () => {
      const field = await startWith("x");
      await emitPartial("y", 1);
      expect(field).toHaveValue("x y");
      cleanup();
      await waitFor(() => expect(cancelVoiceRecording).toHaveBeenCalled());
      expect(stopVoiceRecording).not.toHaveBeenCalled();
    });
  });
});
