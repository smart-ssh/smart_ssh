// Spec 0050, Teil 2 ("Testbarkeit"): "Format-Hinweis: falscher Präfix →
// Warnung, Speichern trotzdem möglich; generischer Provider → keine
// Warnung."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testAiProviderCredentials } from "../api";
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
      expect(screen.getByText("✗ Authentifizierung fehlgeschlagen")).toBeInTheDocument(),
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
