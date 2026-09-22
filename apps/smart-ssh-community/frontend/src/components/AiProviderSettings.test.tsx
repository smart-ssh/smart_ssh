// Spec 0050, Teil 2 ("Testbarkeit"): "Format-Hinweis: falscher Präfix →
// Warnung, Speichern trotzdem möglich; generischer Provider → keine
// Warnung."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { addAiProvider, testAiProviderCredentials } from "../api";
import { testI18n } from "../testI18n";
import { AiProviderSettings } from "./AiProviderSettings";

vi.mock("../api", () => ({
  listAiProviders: vi.fn(() => Promise.resolve([])),
  addAiProvider: vi.fn(() => Promise.resolve("new-id")),
  deleteAiProvider: vi.fn(),
  discoverModels: vi.fn(),
  fetchAttestationInfo: vi.fn(),
  setActiveAiProvider: vi.fn(),
  testAiProviderCredentials: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
  saveRiskClassifierSettings: vi.fn(),
}));

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
