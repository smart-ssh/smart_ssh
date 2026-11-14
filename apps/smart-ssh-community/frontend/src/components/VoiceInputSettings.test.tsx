// Issue #1299, Spec 0108 §2.1, §5.8: the settings list the models the backend offers.
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { listSpeechModels, setSpeechLanguage, setSelectedSpeechModel } from "../api/voiceApi";
import { VoiceInputSettings } from "./VoiceInputSettings";
import { cancelSpeechModelDownload, startSpeechModelDownload } from "../api/voiceApi";
import type { Activity } from "../api/activitiesApi";

let activitiesNow: Activity[] = [];
vi.mock("../hooks/useActivities", () => ({ useActivities: () => activitiesNow }));

const WHISPER = [
  { id: "base", engine: "whisper", total_size: 147_951_465, source_host: "huggingface.co", source_repo: "ggerganov/whisper.cpp" },
  { id: "small", engine: "whisper", total_size: 487_601_967, source_host: "huggingface.co", source_repo: "ggerganov/whisper.cpp" },
];
const PARAKEET = {
  id: "parakeet_v3", engine: "parakeet", total_size: 670_619_706,
  source_host: "huggingface.co", source_repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
};

let selected = "base";
vi.mock("../api/voiceApi", async (importActual) => ({
  ...(await importActual<typeof import("../api/voiceApi")>()),
  listSpeechModels: vi.fn(),
  getSelectedSpeechModel: vi.fn(() => Promise.resolve(selected)),
  setSelectedSpeechModel: vi.fn(() => Promise.resolve()),
  getSpeechLanguage: vi.fn(() => Promise.resolve("de")),
  setSpeechLanguage: vi.fn(() => Promise.resolve()),
  startSpeechModelDownload: vi.fn(() => Promise.resolve()),
  cancelSpeechModelDownload: vi.fn(() => Promise.resolve()),
  getSpeechModelStatus: vi.fn((model: string) => Promise.resolve({ model, state: "not_downloaded" })),
  onSpeechModelStatus: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("../settings/voiceInputSettings", async (importActual) => ({
  ...(await importActual<typeof import("../settings/voiceInputSettings")>()),
  loadVoiceInputEnabled: vi.fn(() => Promise.resolve(true)),
}));

const HINT = /Parakeet erkennt die Sprache automatisch/;

function renderSettings() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <VoiceInputSettings />
    </I18nextProvider>,
  );
}

describe("VoiceInputSettings models", () => {
  beforeEach(async () => {
    selected = "base";
    await testI18n.changeLanguage("de");
    vi.mocked(listSpeechModels).mockResolvedValue([...WHISPER, PARAKEET] as never);
  });
  afterEach(cleanup);

  it("lists the models from the backend with size and the Parakeet description", async () => {
    renderSettings();
    expect(await screen.findAllByRole("radio")).toHaveLength(3);
    const parakeet = screen.getByLabelText(/Parakeet v3 \(NVIDIA\)/);
    expect(parakeet.closest("label")).toHaveTextContent("640");
    expect(parakeet.closest("label")).toHaveTextContent("erkennt die Sprache automatisch");
    expect(screen.getByLabelText(/^Base/).closest("label")).toHaveTextContent("142");
  });

  it("shows no Parakeet entry or hint when the backend does not list it", async () => {
    vi.mocked(listSpeechModels).mockResolvedValue(WHISPER as never);
    selected = "parakeet_v3";
    renderSettings();
    expect(await screen.findAllByRole("radio")).toHaveLength(2);
    expect(screen.queryByLabelText(/Parakeet/)).toBeNull();
    expect(screen.queryByText(HINT)).toBeNull();
  });

  it("shows the language hint only with Parakeet selected and keeps the language", async () => {
    renderSettings();
    await screen.findAllByRole("radio");
    expect(screen.queryByText(HINT)).toBeNull();
    fireEvent.click(screen.getByLabelText(/Parakeet v3 \(NVIDIA\)/));
    expect(await screen.findByText(HINT)).toBeTruthy();
    expect(vi.mocked(setSelectedSpeechModel)).toHaveBeenCalledWith("parakeet_v3");
    expect(vi.mocked(setSpeechLanguage)).not.toHaveBeenCalled();
    const select = screen.getByRole("combobox");
    expect((select as HTMLSelectElement).value).toBe("de");
    expect(within(select).getAllByRole("option").length).toBeGreaterThan(1);
    fireEvent.click(screen.getByLabelText(/^Base/));
    await waitFor(() => expect(screen.queryByText(HINT)).toBeNull());
    expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe("de");
  });
});

describe("VoiceInputSettings running download (issue #1326)", () => {
  beforeEach(async () => {
    selected = "base";
    await testI18n.changeLanguage("de");
    vi.mocked(listSpeechModels).mockResolvedValue(WHISPER as never);
    vi.clearAllMocks();
    vi.mocked(listSpeechModels).mockResolvedValue(WHISPER as never);
    activitiesNow = [
      {
        id: "a1", kind: "speech-model", key: "base", titleKey: "activities.speechModelTitle",
        titleParams: { name: "Base" }, state: "running", done: 50, total: 100, statusKey: null,
        errorCode: null, errorMessage: null, cancellable: true, startedAt: 1, dismissed: false,
      },
    ];
  });
  afterEach(() => {
    cleanup();
    activitiesNow = [];
  });

  it("shows a running download from the activity store with cancel and no second download", async () => {
    renderSettings();
    await screen.findAllByRole("radio");
    const row = screen.getByLabelText(/^Base/).closest("div") as HTMLElement;
    expect(within(row).getByText(/50\s*%/)).toBeTruthy();
    expect(within(row).queryByRole("button", { name: "Herunterladen" })).toBeNull();
    // The other model is still offered.
    const small = screen.getByLabelText(/^Small/).closest("div") as HTMLElement;
    expect(within(small).getByRole("button", { name: "Herunterladen" })).toBeTruthy();
    expect(startSpeechModelDownload).not.toHaveBeenCalled();
    fireEvent.click(within(row).getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(cancelSpeechModelDownload).toHaveBeenCalledWith("base"));
  });
});
