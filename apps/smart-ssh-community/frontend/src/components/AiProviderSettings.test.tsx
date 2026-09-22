// Spec 0050, Teil 2 ("Testbarkeit"): "Format-Hinweis: falscher Präfix →
// Warnung, Speichern trotzdem möglich; generischer Provider → keine
// Warnung."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addAiProvider,
  discoverModels,
  listAiProviders,
  setActiveAiProvider,
  testAiProviderCredentials,
} from "../api";
import type { AiProviderConfigDto } from "../types";
import { testI18n } from "../testI18n";
import { AiProviderSettings } from "./AiProviderSettings";

vi.mock("../api", () => ({
  listAiProviders: vi.fn(() => Promise.resolve([])),
  addAiProvider: vi.fn(() => Promise.resolve("new-id")),
  deleteAiProvider: vi.fn(),
  // Spec 0069, Teil B1: jeder dieser Tests mountet die Komponente ohne
  // vorhandenen Ollama-Provider — das löst die automatische Probe aus
  // (`runOllamaProbe`, ruft `discoverModels` auf). Ein bewusst
  // fehlschlagender Default (statt eines unkonfigurierten `vi.fn()`, der
  // `undefined` zurückgäbe und `models.length` crashen ließe) hält diese
  // Hintergrund-Probe in Tests, die sie nicht selbst konfigurieren, aus
  // dem Weg — Ergebnis "otherError", also keine Karte (s. Spec 0069 §3.B3
  // "jeder andere Fehler → keine Karte").
  discoverModels: vi.fn(() => Promise.reject(new Error("not configured in this test"))),
  fetchAttestationInfo: vi.fn(),
  setActiveAiProvider: vi.fn(),
  testAiProviderCredentials: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
  // Spec 0069, Teil B: echte Implementierung (wie in `ServerList.test.tsx`
  // bereits etabliert) statt einer Attrappe — `runOllamaProbe` braucht das
  // tatsächliche `code`-Extraktionsverhalten.
  commandErrorCode: (err: unknown) => {
    if (typeof err === "object" && err !== null && "code" in err) {
      const code = (err as { code: unknown }).code;
      if (typeof code === "string") return code;
    }
    return null;
  },
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
  saveRiskClassifierSettings: vi.fn(),
}));

// Spec 0069, Teil B, Tests 21/24/25: ohne diesen Reset würden sich
// Aufrufe (`addAiProvider`/`discoverModels`/`setActiveAiProvider`) über
// Testfälle hinweg auf demselben Mock ansammeln — die neuen Tests unten
// prüfen exakte Aufrufzahlen bzw. "nie aufgerufen", was nur mit einem
// sauberen Mock-Zustand pro Test aussagekräftig ist. Löscht nur die
// Aufruf-Historie (`mock.calls`), nicht die in `vi.mock(...)` oben
// hinterlegten Standard-Implementierungen.
beforeEach(() => {
  vi.clearAllMocks();
});

function renderForm() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <AiProviderSettings onProvidersChanged={vi.fn()} />
    </I18nextProvider>,
  );
}

describe("AiProviderSettings API-key format hint (Spec 0050, Teil 2)", () => {
  it("shows a non-blocking hint for an Anthropic key with the wrong prefix, and the submit button stays enabled", () => {
    renderForm();

    // Default provider type is "openai" (emptyForm()); switch to Anthropic.
    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "totally-wrong-format" },
    });

    expect(screen.getByText(/Sieht nicht wie ein Anthropic-Key aus/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hinzufügen" })).not.toBeDisabled();
  });

  it("shows no hint once the key matches the expected prefix", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "sk-ant-abc123" },
    });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });

  it("shows no hint for a generic OpenAI-compatible provider regardless of key content", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), {
      target: { value: "generic_openai_compatible" },
    });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "whatever-format-this-self-hosted-endpoint-uses" },
    });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });

  it("shows no hint while the key field is still empty", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });
});

// Spec 0050, Teil 3 ("Testbarkeit"): "Testen-Button: gültiger Key →
// 'gültig', falscher → 'Auth fehlgeschlagen', unerreichbar → 'nicht
// erreichbar'" — the Rust-side three-way classification itself is already
// covered against a mock AiProvider (crates/app-shell/src/commands.rs's
// credential_test_tests); these tests cover the other half: that the
// button actually calls testAiProviderCredentials with the current,
// unsaved form data and renders each of the three outcomes distinctly.
describe("AiProviderSettings credentials test button (Spec 0050, Teil 3)", () => {
  it("is disabled until an API key is entered", () => {
    renderForm();

    expect(screen.getByRole("button", { name: "Zugangsdaten testen" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    expect(screen.getByRole("button", { name: "Zugangsdaten testen" })).not.toBeDisabled();
  });

  /** Spec-Reviewer-Fund (Spec 0050, Review dieses Schritts): ohne diese
   * Sperre könnte ein Klick vor dem Ausfüllen der Base-URL den API-Key an
   * den Backend-Fallback (OpenAI) statt an den gewählten,
   * selbstgehosteten Endpunkt schicken — dieselbe Gefahr, gegen die der
   * "Modelle laden"-Button bereits abgesichert ist. */
  it("stays disabled for a generic OpenAI-compatible provider until the base URL is filled in, even with a key entered", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), {
      target: { value: "generic_openai_compatible" },
    });
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    expect(screen.getByRole("button", { name: "Zugangsdaten testen" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Base-URL"), {
      target: { value: "https://my-gateway.example/v1" },
    });

    expect(screen.getByRole("button", { name: "Zugangsdaten testen" })).not.toBeDisabled();
  });

  it("shows a valid result", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({ kind: "valid" });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));

    await waitFor(() => expect(screen.getByText("✓ Zugangsdaten gültig")).toBeInTheDocument());
    expect(testAiProviderCredentials).toHaveBeenCalledWith(
      expect.objectContaining({ apiKey: "sk-abc" }),
    );
  });

  it("shows an authentication-failed result", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({ kind: "authenticationFailed" });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-wrong" } });

    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));

    await waitFor(() =>
      expect(
        screen.getByText("✗ Authentifizierung fehlgeschlagen – API-Key prüfen"),
      ).toBeInTheDocument(),
    );
  });

  it("shows an unreachable result including the message", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({
      kind: "unreachable",
      message: "connection refused",
    });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));

    await waitFor(() =>
      expect(
        screen.getByText("✗ Provider nicht erreichbar: connection refused"),
      ).toBeInTheDocument(),
    );
  });

  // Spec 0069, Teil A5, Test 19: `unreachable` mit `AI_RATE_LIMITED` zeigt
  // den eigenen "testResultRateLimited"-Text (nicht den generischen
  // "Provider nicht erreichbar: ..."-Fallback — die Zugangsdaten sind ja
  // vermutlich gültig, nur gerade gedrosselt).
  it("shows the rate-limited-specific text for AI_RATE_LIMITED, not the generic unreachable text", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({
      kind: "unreachable",
      message: "HTTP 429: too many requests",
      code: "AI_RATE_LIMITED",
    });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));

    await waitFor(() =>
      expect(
        screen.getByText(
          "Zugangsdaten vermutlich gültig, der Anbieter drosselt aber gerade. In einer Minute erneut testen.",
        ),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText(/Provider nicht erreichbar/)).not.toBeInTheDocument();
  });

  // Spec 0069, Teil A5, Test 19: mit `AI_MODEL_NOT_FOUND` zeigt die Box
  // den übersetzten Text statt des rohen `message`-Strings.
  it("shows the translated text for AI_MODEL_NOT_FOUND instead of the raw message", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({
      kind: "unreachable",
      message: "HTTP 404: model_not_found",
      code: "AI_MODEL_NOT_FOUND",
    });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });

    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));

    await waitFor(() =>
      expect(screen.getByText(/kennt dieses Modell nicht/)).toBeInTheDocument(),
    );
    expect(screen.queryByText(/HTTP 404: model_not_found/)).not.toBeInTheDocument();
  });

  it("clears a stale result once the key is edited again", async () => {
    vi.mocked(testAiProviderCredentials).mockResolvedValue({ kind: "valid" });
    renderForm();
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Zugangsdaten testen" }));
    await waitFor(() => expect(screen.getByText("✓ Zugangsdaten gültig")).toBeInTheDocument());

    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc-changed" } });

    expect(screen.queryByText("✓ Zugangsdaten gültig")).not.toBeInTheDocument();
  });
});

// Spec 0056, Teil 3 ("Testbarkeit"): "Komponententests für die Präsenz der
// neuen Struktur-Elemente" — die drei neu eingeführten Karten-Abschnitte
// ("Konfigurierte Provider" / "Risiko-Indikatoren — KI-Zweitmeinung" /
// "Provider hinzufügen") müssen alle gerendert werden, inklusive der
// dafür neu eingeführten Überschrift "Konfigurierte Provider" (vorher gab
// es dafür gar keine eigene Überschrift).
describe("AiProviderSettings visual structure (Spec 0056, Teil 3)", () => {
  it("renders all three card sections with their headings", () => {
    renderForm();

    expect(screen.getByRole("heading", { name: "Konfigurierte Provider" })).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Risiko-Indikatoren — KI-Zweitmeinung" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Provider hinzufügen" })).toBeInTheDocument();
  });
});

// Spec 0065, Teil 4: optionaler max_tokens-Override im "Erweitert"-Bereich.
describe("AiProviderSettings max_tokens override (Spec 0065, Teil 4)", () => {
  function openAdvanced() {
    fireEvent.click(screen.getByRole("button", { name: /Erweitert/ }));
  }

  it("defaults to empty (Automatisch) and is not sent as 0", async () => {
    renderForm();
    openAdvanced();

    const field = screen.getByLabelText("Max. Antwortlänge (Tokens)", { exact: false });
    expect(field).toHaveValue(null);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Prov" } });
    fireEvent.change(screen.getByLabelText("Modell"), { target: { value: "gpt-4o" } });
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Hinzufügen" }));

    await waitFor(() => expect(addAiProvider).toHaveBeenCalled());
    expect(vi.mocked(addAiProvider).mock.calls.at(-1)?.[0].maxTokensOverride).toBeNull();
  });

  it("sends the entered value as a number", async () => {
    renderForm();
    openAdvanced();

    fireEvent.change(screen.getByLabelText("Max. Antwortlänge (Tokens)", { exact: false }), {
      target: { value: "20000" },
    });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Prov" } });
    fireEvent.change(screen.getByLabelText("Modell"), { target: { value: "gpt-4o" } });
    fireEvent.change(screen.getByLabelText("API-Key"), { target: { value: "sk-abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Hinzufügen" }));

    await waitFor(() => expect(addAiProvider).toHaveBeenCalled());
    expect(vi.mocked(addAiProvider).mock.calls.at(-1)?.[0].maxTokensOverride).toBe(20000);
  });

  it("shows a validation hint and disables submit for 0", () => {
    renderForm();
    openAdvanced();

    fireEvent.change(screen.getByLabelText("Max. Antwortlänge (Tokens)", { exact: false }), {
      target: { value: "0" },
    });

    expect(screen.getByText("Max. Antwortlänge muss größer als 0 sein")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hinzufügen" })).toBeDisabled();
  });
});

// Spec 0069, Teil B: Ollama-Erkennung.
function ollamaProvider(overrides: Partial<AiProviderConfigDto> = {}): AiProviderConfigDto {
  return {
    id: "existing-ollama",
    providerType: "ollama",
    displayName: "Ollama (lokal)",
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "llama3",
    supportsNativeToolCalling: true,
    isActive: false,
    extraHeaders: [],
    attestationUrl: null,
    maxTokensOverride: null,
    ...overrides,
  };
}

function activeAnthropicProvider(): AiProviderConfigDto {
  return {
    id: "existing-anthropic",
    providerType: "anthropic",
    displayName: "Claude",
    baseUrl: null,
    model: "claude-sonnet",
    supportsNativeToolCalling: true,
    isActive: true,
    extraHeaders: [],
    attestationUrl: null,
    maxTokensOverride: null,
  };
}

describe("AiProviderSettings Ollama probe trigger (Spec 0069, Teil B1, Test 21)", () => {
  it("mounts without an Ollama provider -> exactly one discoverModels call against 127.0.0.1:11434/v1", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce([]);

    renderForm();

    await waitFor(() => expect(discoverModels).toHaveBeenCalledTimes(1));
    const [config] = vi.mocked(discoverModels).mock.calls[0];
    expect(config.providerType).toBe("ollama");
    expect(config.baseUrl).toBe("http://127.0.0.1:11434/v1");
  });

  it("mounts with an existing Ollama provider -> no discoverModels call", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([ollamaProvider()]);

    renderForm();

    // Auf das Laden der Liste warten (sichtbar an der Provider-Zeile),
    // damit der Test nicht zufällig vor dem Effekt endet.
    await screen.findByText("Ollama (lokal)");
    expect(discoverModels).not.toHaveBeenCalled();
  });

  it('"Erneut suchen" triggers another discoverModels call', async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce([]); // -> empty state, shows "Erneut suchen"

    renderForm();
    await waitFor(() => expect(discoverModels).toHaveBeenCalledTimes(1));
    await screen.findByText("Erneut suchen");

    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3"]);
    fireEvent.click(screen.getByRole("button", { name: "Erneut suchen" }));

    await waitFor(() => expect(discoverModels).toHaveBeenCalledTimes(2));
  });
});

describe("AiProviderSettings Ollama probe results (Spec 0069, Teil B3, Test 23)", () => {
  it("models found -> suggestion card with a model select", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3", "mistral"]);

    renderForm();

    await screen.findByText("Ollama läuft auf diesem Rechner.");
    expect(screen.getByRole("button", { name: "Ollama übernehmen" })).toBeInTheDocument();
  });

  it("empty model list -> pull instructions, no suggestion card", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce([]);

    renderForm();

    await screen.findByText("Ollama läuft, aber es ist noch kein Modell geladen.");
    expect(screen.queryByRole("button", { name: "Ollama übernehmen" })).not.toBeInTheDocument();
  });

  it("AI_LOCAL_PROVIDER_UNREACHABLE with no providers configured -> install instructions", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockRejectedValueOnce({ code: "AI_LOCAL_PROVIDER_UNREACHABLE" });

    renderForm();

    await screen.findByText("Kein lokales Ollama gefunden (Standard-Port 11434).");
  });

  it("AI_LOCAL_PROVIDER_UNREACHABLE with an already-configured (non-Ollama) provider -> no card", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([activeAnthropicProvider()]);
    vi.mocked(discoverModels).mockRejectedValueOnce({ code: "AI_LOCAL_PROVIDER_UNREACHABLE" });

    renderForm();

    await waitFor(() => expect(discoverModels).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByText("Kein lokales Ollama gefunden (Standard-Port 11434)."),
    ).not.toBeInTheDocument();
  });

  it("any other error -> no card at all (stays silent)", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockRejectedValueOnce({ code: "AI_NETWORK_ERROR" });

    renderForm();

    await waitFor(() => expect(discoverModels).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByText("Kein lokales Ollama gefunden (Standard-Port 11434)."),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Ollama läuft auf diesem Rechner.")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Ollama läuft, aber es ist noch kein Modell geladen."),
    ).not.toBeInTheDocument();
  });
});

describe('AiProviderSettings "Ollama übernehmen" (Spec 0069, Teil B4, Test 24/25)', () => {
  it("without an active provider -> addAiProvider with the expected fields, then setActiveAiProvider", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3"]);
    vi.mocked(addAiProvider).mockResolvedValueOnce("new-ollama-id");

    renderForm();
    await screen.findByText("Ollama läuft auf diesem Rechner.");

    fireEvent.click(screen.getByRole("button", { name: "Ollama übernehmen" }));

    await waitFor(() => expect(addAiProvider).toHaveBeenCalledTimes(1));
    expect(addAiProvider).toHaveBeenCalledWith(
      expect.objectContaining({
        providerType: "ollama",
        baseUrl: "http://127.0.0.1:11434/v1",
        model: "llama3",
        apiKey: "ollama-no-key",
      }),
    );
    await waitFor(() => expect(setActiveAiProvider).toHaveBeenCalledWith("new-ollama-id"));
  });

  it("with an already-active provider -> addAiProvider but no setActiveAiProvider", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([activeAnthropicProvider()]);
    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3"]);
    vi.mocked(addAiProvider).mockResolvedValueOnce("new-ollama-id");

    renderForm();
    await screen.findByText("Ollama läuft auf diesem Rechner.");

    fireEvent.click(screen.getByRole("button", { name: "Ollama übernehmen" }));

    await waitFor(() => expect(addAiProvider).toHaveBeenCalledTimes(1));
    // Kein setActiveAiProvider -- gibt genug Zeit für einen fälschlichen
    // Aufruf, bevor negativ geprüft wird.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(setActiveAiProvider).not.toHaveBeenCalled();
  });

  it("addAiProvider failing -> no setActiveAiProvider call, error shown like any other add failure", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3"]);
    vi.mocked(addAiProvider).mockRejectedValueOnce(new Error("boom"));

    renderForm();
    await screen.findByText("Ollama läuft auf diesem Rechner.");

    fireEvent.click(screen.getByRole("button", { name: "Ollama übernehmen" }));

    await waitFor(() => expect(screen.getByText("Error: boom")).toBeInTheDocument());
    expect(setActiveAiProvider).not.toHaveBeenCalled();
  });

  // Test 25: ohne Klick wird nie addAiProvider aufgerufen, auch nicht nach
  // einer erfolgreichen Probe.
  it("never calls addAiProvider without a click, even after a successful probe", async () => {
    vi.mocked(listAiProviders).mockResolvedValueOnce([]);
    vi.mocked(discoverModels).mockResolvedValueOnce(["llama3"]);

    renderForm();
    await screen.findByText("Ollama läuft auf diesem Rechner.");

    expect(addAiProvider).not.toHaveBeenCalled();
  });
});
