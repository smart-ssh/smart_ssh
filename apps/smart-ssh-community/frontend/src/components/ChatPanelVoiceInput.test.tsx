// Spec 0055, Teil 1: Enter sendet; Shift+Enter fügt einen
// Zeilenumbruch ein; das Feld wächst bis zu einem Maximum, dann scrollt es intern. Eigene Datei
// statt einer Erweiterung von `ChatPanel.test.tsx` — jene Datei testet
// bewusst nur `ChatItemView` isoliert (kein API-Mocking nötig), das
// Eingabefeld lebt aber in `ChatPanel` selbst, das für einen sinnvollen
// Render deutlich mehr Abhängigkeiten mocken muss.
import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendChatMessage } from "../api/api";
import { renderChatPanel } from "../test/renderChatPanel";
import {
  cancelSpeechModelDownload,
  cancelVoiceRecording,
  getSelectedSpeechModel,
  getSpeechModelStatus,
  startSpeechModelDownload,
  startVoiceRecording,
  stopVoiceRecording,
} from "../api/voiceApi";
import { loadVoiceInputEnabled } from "../settings/voiceInputSettings";

// jsdom implementiert `Element.scrollTo` nicht — `ChatPanel`s
// Auto-Scroll-Effekt (unabhängig von dieser Spec) ruft es bei jeder
// `items`-Änderung auf.
import "../test/scrollTo";

vi.mock("../api/api", async () => {
  const { anthropicProvider } = await import("../test/fixtures");
  const { createApiMock } = await import("../test/apiMock");
  return createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([anthropicProvider()])),
  });
});

vi.mock("../api/events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../settings/riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

const statusListeners: Array<(ev: unknown) => void> = [];
const partialListeners: Array<
  (ev: { text: string; revision: number }) => void
> = [];
const emitStatus = (ev: unknown) =>
  act(() => statusListeners.forEach((h) => h(ev)));
const emitPartial = (text: string, revision: number) =>
  act(() => partialListeners.forEach((h) => h({ text, revision })));

vi.mock("../api/voiceApi", async (importActual) => ({
  ...(await importActual<typeof import("../api/voiceApi")>()),
  getSelectedSpeechModel: vi.fn(() => Promise.resolve("base")),
  listSpeechModels: vi.fn(() =>
    Promise.resolve([
      { id: "base", engine: "whisper", total_size: 147_951_465, source_host: "huggingface.co", source_repo: "ggerganov/whisper.cpp" },
      { id: "parakeet_v3", engine: "parakeet", total_size: 670_619_706, source_host: "huggingface.co", source_repo: "istupakov/parakeet-tdt-0.6b-v3-onnx" },
    ]),
  ),
  getSpeechModelStatus: vi.fn(() =>
    Promise.resolve({ model: "base", state: "ready" }),
  ),
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
  onSpeechPartialText: vi.fn(
    (h: (ev: { text: string; revision: number }) => void) => {
      partialListeners.push(h);
      return Promise.resolve(() => {});
    },
  ),
}));

vi.mock("../settings/voiceInputSettings", async (importActual) => ({
  ...(await importActual<typeof import("../settings/voiceInputSettings")>()),
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
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "ready",
    } as never);
    vi.mocked(loadVoiceInputEnabled).mockResolvedValue(true);
  });

  it("inserts the transcript into the draft without sending", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(
      await screen.findByRole("button", { name: /Aufnahme beenden/ }),
    );
    await waitFor(() => expect(field).toHaveValue("hallo welt"));
    expect(sendChatMessage).not.toHaveBeenCalled();
  });

  it("appends the transcript to an existing draft, separated by a space", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "ls und" } });
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(
      await screen.findByRole("button", { name: /Aufnahme beenden/ }),
    );
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
    expect(
      await screen.findByRole("button", { name: START }),
    ).toBeInTheDocument();
    expect(field).toHaveValue("entwurf");
  });

  it("the shortcut starts and stops the recording while the panel has focus", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    await screen.findByRole("button", { name: START });
    field.focus();
    fireEvent.keyDown(field, {
      key: "M",
      code: "KeyM",
      ctrlKey: true,
      shiftKey: true,
    });
    await waitFor(() => expect(startVoiceRecording).toHaveBeenCalledTimes(1));
    await screen.findByRole("button", { name: /Aufnahme beenden/ });
    fireEvent.keyDown(field, {
      key: "M",
      code: "KeyM",
      ctrlKey: true,
      shiftKey: true,
    });
    await waitFor(() => expect(stopVoiceRecording).toHaveBeenCalledTimes(1));
  });

  it("without a model the first click opens the consent dialog; cancel makes no download", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "not_downloaded",
    } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(dialog).toHaveTextContent("142"));
    expect(dialog).toHaveTextContent("huggingface.co");
    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(startSpeechModelDownload).not.toHaveBeenCalled();
    expect(cancelSpeechModelDownload).not.toHaveBeenCalled();
    expect(startVoiceRecording).not.toHaveBeenCalled();
    expect(getSelectedSpeechModel).toHaveBeenCalled();
  });

  it("consent dialog for Parakeet names model, size, source repo and license", async () => {
    vi.mocked(getSelectedSpeechModel).mockResolvedValueOnce("parakeet_v3");
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "parakeet_v3",
      state: "not_downloaded",
    } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(dialog).toHaveTextContent("Parakeet v3 (NVIDIA)"));
    expect(dialog).toHaveTextContent("640");
    expect(dialog).toHaveTextContent("huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx");
    expect(dialog).toHaveTextContent("CC-BY-4.0, NVIDIA");
  });

  it("confirming downloads the model and starts recording only once it is ready", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "not_downloaded",
    } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Herunterladen" }),
    );
    await waitFor(() =>
      expect(startSpeechModelDownload).toHaveBeenCalledWith("base"),
    );
    expect(startVoiceRecording).not.toHaveBeenCalled();
    await waitFor(() => expect(statusListeners.length).toBeGreaterThan(0));
    await emitStatus({
      model: "base",
      state: "downloading",
      done: 5,
      total: 10,
    });
    await screen.findByRole("progressbar");
    await emitStatus({ model: "base", state: "ready" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(startVoiceRecording).toHaveBeenCalledTimes(1);
  });

  it("cancelling during the download cancels it and does not record", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "not_downloaded",
    } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Herunterladen" }),
    );
    await waitFor(() => expect(startSpeechModelDownload).toHaveBeenCalled());
    // The download button disappears once the dialog switched to its progress state.
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Herunterladen" }),
      ).toBeNull(),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Abbrechen" }));
    await waitFor(() =>
      expect(cancelSpeechModelDownload).toHaveBeenCalledWith("base"),
    );
    expect(startVoiceRecording).not.toHaveBeenCalled();
  });

  it("closing the dialog during the download keeps it running; reopening shows it without a second start (issue #1326)", async () => {
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "not_downloaded",
    } as never);
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Herunterladen" }),
    );
    await waitFor(() => expect(startSpeechModelDownload).toHaveBeenCalledTimes(1));
    await screen.findByRole("progressbar");
    // Esc closes the dialog but must not cancel the download.
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(cancelSpeechModelDownload).not.toHaveBeenCalled();
    // Reopen while the backend reports the running download.
    vi.mocked(getSpeechModelStatus).mockResolvedValue({
      model: "base",
      state: "downloading",
      done: 5,
      total: 10,
    } as never);
    fireEvent.click(await screen.findByRole("button", { name: START }));
    const bar = await screen.findByRole("progressbar");
    expect(bar).toHaveAttribute("value", "5");
    expect(screen.queryByRole("button", { name: "Herunterladen" })).toBeNull();
    expect(startSpeechModelDownload).toHaveBeenCalledTimes(1);
  });

  it("shows the translated message for a backend error and returns to idle", async () => {
    vi.mocked(startVoiceRecording).mockRejectedValueOnce({
      code: "speech_permission_denied",
      message: "x",
    });
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
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: START })).toBeNull(),
    );
    expect(loadVoiceInputEnabled).toHaveBeenCalled();
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
      expect((field as HTMLTextAreaElement).selectionStart).toBe(
        "ls und hallo welt".length,
      );
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
      vi.mocked(stopVoiceRecording).mockRejectedValue({
        code: "speech_transcription_failed",
      });
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
